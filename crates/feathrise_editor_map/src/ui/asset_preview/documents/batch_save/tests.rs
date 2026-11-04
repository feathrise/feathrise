//! 真实临时文件验证批量范围、独立失败、保存点、代次隔离与单飞，不改正式资产。

use super::super::{io, resources::DocumentState, session, types::*};
use super::{control, resources::DocumentBatchSaveState, types::*};
use crate::{
    asset_catalog::document_format, ui::vegetation_preset_editor::VegetationPresetEditorState,
};
use bevy::prelude::*;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "rpg-batch-save-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        Self { root }
    }
    fn doc(&self, path: &str, original: &str, draft: &str) -> Document {
        std::fs::write(self.root.join(path), original).unwrap();
        let (disk, source) = io::load(&self.root, path).unwrap();
        let mut doc = Document {
            path: path.into(),
            format: document_format(path).unwrap(),
            generation: 1,
            disk,
            pages: source.pages,
            source: source.text,
            revision: 0,
            saved_revision: 0,
            history: default(),
            preview: Arc::new(Vec::new()),
            preview_revision: None,
            preview_clipped: false,
            mode: ViewMode::Edit,
            preview_scroll: Vec2::ZERO,
            confirm_reload: false,
            external: default(),
            status: String::new(),
        };
        doc.edit(draft);
        doc
    }
    fn app(&self, docs: Vec<Document>) -> App {
        let mut app = App::new();
    app.insert_resource(feathrise_editor_documents::content_operations());
        app.add_plugins(MinimalPlugins)
            .insert_resource(DocumentState {
                root: self.root.clone(),
                active: docs.first().map(|doc| doc.path.clone()),
                documents: docs,
                ..default()
            })
            .init_resource::<DocumentBatchSaveState>()
            .add_systems(Update, session::drive_tasks);
        app
    }
    fn read(&self, path: &str) -> String {
        std::fs::read_to_string(self.root.join(path)).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        for entry in std::fs::read_dir(&self.root).unwrap().flatten() {
            if let Ok(metadata) = entry.metadata() {
                let mut permissions = metadata.permissions();
                if permissions.readonly() {
                    permissions.set_readonly(false);
                    let _ = std::fs::set_permissions(entry.path(), permissions);
                }
            }
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn request(app: &mut App, scope: SaveScope) {
    app.world_mut()
        .resource_scope(|world, mut state: Mut<DocumentBatchSaveState>| {
            control::request(
                &mut state,
                &mut world.resource_mut::<DocumentState>(),
                scope,
                feathrise_editor_documents::content_operations(),
            );
        });
}
fn settle(app: &mut App) {
    for _ in 0..3000 {
        app.update();
        if app.world().resource::<DocumentState>().io.is_none() {
            return;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    panic!("批量文件任务未在3秒内完成");
}

#[test]
fn batch_save_all_includes_parked_and_closed_drafts_but_never_clean_files() {
    let f = Fixture::new();
    let docs = vec![
        f.doc("a.md", "旧甲", "新甲"),
        f.doc("b.rs", "old", "new"),
        f.doc("clean.toml", "key=1", "key=1"),
    ];
    let mut app = f.app(docs);
    let mut workspace = VegetationPresetEditorState::default();
    {
        let mut docs = app.world_mut().resource_mut::<DocumentState>();
        docs.expand("a.md".into(), &mut workspace).unwrap();
        docs.expand("b.rs".into(), &mut workspace).unwrap();
        docs.close_all_tabs(); // 已关闭的文档页仍有草稿，保存全部不能遗漏。
    }
    request(&mut app, SaveScope::All);
    settle(&mut app);
    let docs = app.world().resource::<DocumentState>();
    assert_eq!(docs.active.as_deref(), Some("a.md"));
    assert!(docs.documents.iter().all(|doc| !doc.dirty()));
    assert!(docs.documents[0].history.can_undo());
    assert_eq!(f.read("a.md"), "新甲");
    assert_eq!(f.read("b.rs"), "new");
    assert_eq!(f.read("clean.toml"), "key=1");
    let report = app.world().resource::<DocumentBatchSaveState>();
    assert_eq!(report.files.len(), 2);
    assert!(
        report
            .files
            .iter()
            .all(|file| file.outcome == FileOutcome::Saved)
    );
    assert!(!report.running);
}

#[test]
fn batch_save_only_saves_actual_batch_files_using_their_current_manual_edits() {
    let f = Fixture::new();
    let docs = vec![
        f.doc("a.txt", "old", "replaced"),
        f.doc("other.txt", "old", "unrelated"),
        f.doc("clean.txt", "clean", "clean"),
    ];
    let keys = vec![docs[0].source_key(), docs[2].source_key()];
    let mut app = f.app(docs);
    app.world_mut().resource_mut::<DocumentState>().documents[0].edit("手工修改替换结果");
    request(&mut app, SaveScope::Batch(keys));
    settle(&mut app);
    assert_eq!(f.read("a.txt"), "手工修改替换结果");
    assert_eq!(f.read("other.txt"), "old");
    let docs = app.world().resource::<DocumentState>();
    assert!(!docs.documents[0].dirty());
    assert!(docs.documents[1].dirty());
    assert_eq!(
        app.world().resource::<DocumentBatchSaveState>().files[1].outcome,
        FileOutcome::Unmodified
    );
}

#[test]
fn batch_save_invalid_json_external_changes_and_readonly_each_fail_without_stopping_later_files() {
    let f = Fixture::new();
    let docs = vec![
        f.doc("first.md", "old", "saved first"),
        f.doc("bad.JSON", "{\"值\":1}", "{\"值\":}"),
        f.doc("external.toml", "k=1", "k=2"),
        f.doc("readonly.wgsl", "old", "draft"),
        f.doc("last.rs", "old", "saved last"),
    ];
    std::fs::write(f.root.join("external.toml"), "external=3").unwrap();
    let mut permissions = std::fs::metadata(f.root.join("readonly.wgsl"))
        .unwrap()
        .permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(f.root.join("readonly.wgsl"), permissions).unwrap();
    let mut app = f.app(docs);
    request(&mut app, SaveScope::All);
    settle(&mut app);
    assert_eq!(f.read("first.md"), "saved first");
    assert_eq!(f.read("last.rs"), "saved last");
    assert_eq!(f.read("bad.JSON"), "{\"值\":1}");
    assert_eq!(f.read("external.toml"), "external=3");
    assert_eq!(f.read("readonly.wgsl"), "old");
    let report = app.world().resource::<DocumentBatchSaveState>();
    assert_eq!(
        report
            .files
            .iter()
            .filter(|file| file.outcome == FileOutcome::Saved)
            .count(),
        2
    );
    assert_eq!(
        report
            .files
            .iter()
            .filter(|file| file.outcome == FileOutcome::Failed)
            .count(),
        3
    );
    assert!(report.files[1].detail.contains("JSON第1行"));
    assert!(report.files[2].detail.contains("外部修改"));
    assert!(report.files[3].detail.contains("只读"));
    let docs = app.world().resource::<DocumentState>();
    assert!(docs.documents[1..4].iter().all(|doc| doc.dirty()));
    assert!(!docs.documents[4].dirty());
    assert!(std::fs::read_dir(&f.root).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("stage")
    }));
}

#[test]
fn batch_save_keeps_bom_and_mixed_raw_line_endings() {
    let f = Fixture::new();
    let doc = f.doc("mixed.txt", "\u{feff}甲\r\n乙\n丙\r\n", "甲\n新乙\n丙\n");
    let mut app = f.app(vec![doc]);
    request(&mut app, SaveScope::All);
    settle(&mut app);
    assert_eq!(f.read("mixed.txt"), "\u{feff}甲\r\n新乙\n丙\r\n");
}

#[test]
fn batch_save_receipt_keeps_new_input_and_updates_disk_baseline_for_retry() {
    let f = Fixture::new();
    let doc = f.doc("a.txt", "old", "clicked draft");
    let mut app = f.app(vec![doc]);
    request(&mut app, SaveScope::All);
    app.world_mut().resource_mut::<DocumentState>().documents[0].edit("later draft");
    settle(&mut app);
    assert_eq!(f.read("a.txt"), "clicked draft");
    assert!(app.world().resource::<DocumentState>().documents[0].dirty());
    assert!(
        app.world().resource::<DocumentBatchSaveState>().files[0]
            .detail
            .contains("之后的修改")
    );
    request(&mut app, SaveScope::All);
    settle(&mut app);
    assert_eq!(f.read("a.txt"), "later draft");
    assert!(!app.world().resource::<DocumentState>().documents[0].dirty());
}

#[test]
fn batch_save_undo_content_identity_marks_only_saved_snapshot_clean() {
    let f = Fixture::new();
    let mut doc = f.doc("a.txt", "old", "first");
    doc.edit("second");
    doc.travel(false);
    assert_ne!(doc.content_revision(), doc.revision);
    let mut app = f.app(vec![doc]);
    request(&mut app, SaveScope::All);
    settle(&mut app);
    assert_eq!(f.read("a.txt"), "first");
    let mut docs = app.world_mut().resource_mut::<DocumentState>();
    assert!(!docs.documents[0].dirty());
    docs.documents[0].travel(true);
    assert!(docs.documents[0].dirty());
    docs.documents[0].travel(false);
    assert!(!docs.documents[0].dirty());
}

#[test]
fn batch_save_generation_mismatch_reports_failed_scope_and_never_saves_reloaded_file() {
    let f = Fixture::new();
    let mut doc = f.doc("a.txt", "old", "new generation draft");
    let old_key = doc.source_key();
    doc.generation = 2;
    let mut app = f.app(vec![doc]);
    request(&mut app, SaveScope::Batch(vec![old_key]));
    assert!(app.world().resource::<DocumentState>().io.is_none());
    assert_eq!(f.read("a.txt"), "old");
    assert!(app.world().resource::<DocumentState>().documents[0].dirty());
    assert_eq!(
        app.world().resource::<DocumentBatchSaveState>().files[0].outcome,
        FileOutcome::Failed
    );
}

#[test]
fn batch_save_late_reply_cannot_clear_new_generation_or_other_root_draft() {
    for root_changed in [false, true] {
        let f = Fixture::new();
        let doc = f.doc("a.txt", "old", "clicked");
        let mut app = f.app(vec![doc]);
        request(&mut app, SaveScope::All);
        {
            let mut docs = app.world_mut().resource_mut::<DocumentState>();
            if root_changed {
                docs.root = f.root.join("other-assets");
            } else {
                docs.documents[0].generation = 2;
            }
            docs.documents[0].edit("new session draft");
        }
        settle(&mut app);
        assert_eq!(f.read("a.txt"), "clicked");
        assert!(app.world().resource::<DocumentState>().documents[0].dirty());
        assert_eq!(
            app.world().resource::<DocumentState>().documents[0].saved_revision,
            0
        );
    }
}

#[test]
fn batch_save_single_flight_and_pending_load_reject_without_replacing_report_ticket() {
    let f = Fixture::new();
    let doc = f.doc("a.txt", "old", "draft");
    let mut app = f.app(vec![doc]);
    request(&mut app, SaveScope::All);
    let ticket = app.world().resource::<DocumentBatchSaveState>().ticket;
    request(&mut app, SaveScope::All);
    assert_eq!(
        app.world().resource::<DocumentBatchSaveState>().ticket,
        ticket
    );
    assert_eq!(
        app.world().resource::<DocumentBatchSaveState>().files.len(),
        1
    );
    settle(&mut app);
    app.world_mut().resource_mut::<DocumentState>().pending_load = Some(LoadRequest {
        path: "a.txt".into(),
        ticket: 10,
    });
    request(&mut app, SaveScope::All);
    assert_eq!(
        app.world().resource::<DocumentBatchSaveState>().ticket,
        ticket
    );
}

#[test]
fn batch_save_inflight_undone_sessions_are_not_evicted_or_closed_as_unmodified() {
    let f = Fixture::new();
    let docs = (0..8)
        .map(|i| f.doc(&format!("{i}.txt"), "old", "clicked"))
        .collect();
    let mut app = f.app(docs);
    let mut workspace = VegetationPresetEditorState::default();
    for i in 0..8 {
        app.world_mut()
            .resource_mut::<DocumentState>()
            .expand(format!("{i}.txt"), &mut workspace)
            .unwrap();
    }
    request(&mut app, SaveScope::All);
    {
        let mut docs = app.world_mut().resource_mut::<DocumentState>();
        for doc in &mut docs.documents {
            doc.travel(false);
            assert!(!doc.dirty());
        }
        assert_eq!(docs.close_unmodified_tabs(), None);
        assert_eq!(docs.tab_count(), 8);
        docs.close_all_tabs();
        assert!(docs.expand("ninth.txt".into(), &mut workspace).is_err());
        assert_eq!(docs.documents.len(), 8);
    }
    settle(&mut app);
    let docs = app.world().resource::<DocumentState>();
    assert!(
        docs.documents.iter().all(|doc| doc.dirty()),
        "撤销后的内容与刚保存的提交快照不同"
    );
    assert!(docs.saving_batch.is_empty());
}

#[test]
fn batch_save_over_byte_limit_fails_only_that_file() {
    let f = Fixture::new();
    let docs = vec![
        f.doc("big.txt", "old", &"中".repeat(100_000)),
        f.doc("later.txt", "old", "saved"),
    ];
    let mut app = f.app(docs);
    request(&mut app, SaveScope::All);
    settle(&mut app);
    assert_eq!(f.read("big.txt"), "old");
    assert_eq!(f.read("later.txt"), "saved");
    assert!(app.world().resource::<DocumentState>().documents[0].dirty());
    assert!(
        app.world().resource::<DocumentBatchSaveState>().files[0]
            .detail
            .contains("256 KiB")
    );
}

#[test]
fn batch_save_failed_json_can_be_fixed_and_retried_without_saving_clean_success_again() {
    let f = Fixture::new();
    let docs = vec![
        f.doc("bad.json", "{}", "{bad}"),
        f.doc("ok.txt", "old", "saved"),
    ];
    let mut app = f.app(docs);
    request(&mut app, SaveScope::All);
    settle(&mut app);
    app.world_mut().resource_mut::<DocumentState>().documents[0].edit("{\"好\":true}");
    request(&mut app, SaveScope::All);
    settle(&mut app);
    let report = app.world().resource::<DocumentBatchSaveState>();
    assert_eq!(report.files.len(), 1);
    assert_eq!(report.files[0].outcome, FileOutcome::Saved);
    assert_eq!(f.read("bad.json"), "{\"好\":true}");
}

fn install_input(app: &mut App) {
    use crate::resources::{EditorTextInputGuard, MapEditorNumericScrubMouseGuard, MapEditorState};
    app.insert_resource(MapEditorState {
        open: true,
        selected_asset: Some("a.txt".into()),
        ..default()
    })
    .init_resource::<EditorTextInputGuard>()
    .init_resource::<crate::ui::external_files::ExternalEditors>()
    .init_resource::<MapEditorNumericScrubMouseGuard>()
    .init_resource::<VegetationPresetEditorState>()
    .init_resource::<ButtonInput<KeyCode>>()
    .add_message::<super::SaveAllDocuments>()
    .add_systems(Update, control::input);
    app.world_mut().resource_mut::<DocumentState>().visible = true;
}

#[test]
fn batch_save_shortcut_consumes_shift_s_in_document_context_and_respects_input_and_modal_gates() {
    for gate in 0..4 {
        let f = Fixture::new();
        let docs = vec![f.doc("a.txt", "old", "new"), f.doc("b.txt", "old", "new")];
        let mut app = f.app(docs);
        install_input(&mut app);
        match gate {
            1 => {
                app.world_mut()
                    .resource_mut::<crate::resources::EditorTextInputGuard>()
                    .block_shortcuts = true
            }
            2 => app.world_mut().resource_mut::<DocumentState>().visible = false,
            3 => {
                let mut text = bevy::text::EditableText::default();
                text.queue_edit(bevy::text::TextEdit::Insert("待确认采用".into()));
                app.world_mut().spawn(text);
            }
            _ => (),
        }
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.press(KeyCode::ControlRight);
        keys.press(KeyCode::ShiftRight);
        keys.press(KeyCode::KeyS);
        app.update();
        if gate == 0 {
            settle(&mut app);
            assert_eq!(f.read("a.txt"), "new");
            assert_eq!(f.read("b.txt"), "new");
        } else {
            assert!(app.world().resource::<DocumentState>().io.is_none());
            assert_eq!(f.read("a.txt"), "old");
            assert_eq!(f.read("b.txt"), "old");
        }
        if gate != 2 {
            assert!(
                !app.world()
                    .resource::<ButtonInput<KeyCode>>()
                    .just_pressed(KeyCode::KeyS)
            );
        }
    }
}

#[test]
fn batch_save_menu_validated_message_survives_the_same_frame_mouse_capture() {
    let f = Fixture::new();
    let doc = f.doc("a.txt", "old", "menu draft");
    let mut app = f.app(vec![doc]);
    install_input(&mut app);
    app.world_mut()
        .resource_mut::<crate::resources::MapEditorNumericScrubMouseGuard>()
        .advance(true, true, false);
    app.world_mut().write_message(super::SaveAllDocuments);
    app.update();
    settle(&mut app);
    assert_eq!(f.read("a.txt"), "menu draft");
    assert_eq!(app.world().resource::<DocumentBatchSaveState>().ticket, 1);
}

#[test]
fn batch_save_does_not_steal_ctrl_s_from_the_current_file() {
    use crate::ui::sidebars::{EditorRightSidebarTab, MapEditorSidebarState};
    let f = Fixture::new();
    let docs = vec![
        f.doc("a.txt", "old", "current"),
        f.doc("b.txt", "old", "background"),
    ];
    let mut app = f.app(docs);
    install_input(&mut app);
    let mut sidebar = MapEditorSidebarState::default();
    sidebar.right = EditorRightSidebarTab::Assets;
    app.insert_resource(sidebar)
        .init_resource::<bevy::input_focus::InputFocus>()
        .add_systems(
            Update,
            super::super::interaction::actions.before(control::input),
        );
    let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    keys.press(KeyCode::ControlLeft);
    keys.press(KeyCode::KeyS);
    app.update();
    settle(&mut app);
    assert_eq!(f.read("a.txt"), "current");
    assert_eq!(f.read("b.txt"), "old");
    assert_eq!(app.world().resource::<DocumentBatchSaveState>().ticket, 0);
}

#[test]
fn batch_save_waits_for_existing_json_save_intent_instead_of_competing_for_io() {
    let f = Fixture::new();
    let doc = f.doc("a.json", "{}", "{\"value\":1}");
    let mut app = f.app(vec![doc]);
    app.world_mut()
        .resource_mut::<DocumentState>()
        .request_save();
    assert!(super::super::json::save_busy(
        app.world().resource::<DocumentState>()
    ));
    request(&mut app, SaveScope::All);
    assert!(app.world().resource::<DocumentState>().io.is_none());
    assert_eq!(app.world().resource::<DocumentBatchSaveState>().ticket, 0);
    assert_eq!(f.read("a.json"), "{}");
}

#[test]
fn batch_save_report_scroll_clamps_at_scaled_bounds_and_still_owns_wheel_at_the_end() {
    use bevy::{
        input::mouse::{MouseScrollUnit, MouseWheel},
        ui::RelativeCursorPosition,
    };
    let f = Fixture::new();
    let doc = f.doc("a.txt", "old", "old");
    let mut app = f.app(vec![doc]);
    install_input(&mut app);
    app.add_message::<MouseWheel>()
        .add_systems(Update, control::scroll);
    app.world_mut()
        .resource_mut::<DocumentBatchSaveState>()
        .shown = true;
    let view = app
        .world_mut()
        .spawn((
            super::components::SaveReportScroll,
            RelativeCursorPosition {
                cursor_over: true,
                normalized: Some(Vec2::ZERO),
            },
            ComputedNode {
                size: Vec2::new(600.0, 180.0),
                content_size: Vec2::new(600.0, 800.0),
                inverse_scale_factor: 0.5,
                ..default()
            },
            ScrollPosition::default(),
        ))
        .id();
    for _ in 0..2 {
        app.world_mut().write_message(MouseWheel {
            unit: MouseScrollUnit::Line,
            x: 0.0,
            y: -50.0,
            window: Entity::PLACEHOLDER,
            phase: bevy::input::touch::TouchPhase::Moved,
        });
        app.update();
        assert_eq!(app.world().get::<ScrollPosition>(view).unwrap().y, 310.0);
        assert!(
            app.world()
                .resource::<DocumentBatchSaveState>()
                .owns_wheel()
        );
    }
    app.update();
    assert!(
        !app.world()
            .resource::<DocumentBatchSaveState>()
            .owns_wheel()
    );
}
