//! 跨文件搜索真实临时资产、Unicode位置、草稿优先、取消和中央定位身份回归。

use super::super::{constants::MAX_FILE_BYTES, io, navigation, resources::DocumentState, types::*};
use super::{
    components::*, constants::*, control, jump, resources::AssetSearchState, tasks, types::*,
    worker,
};
use crate::{
    resources::*,
    ui::{
        sidebars::{EditorInspectorTarget, EditorNavigatorTab, EditorRightSidebarTab, MapEditorSidebarState},
        vegetation_preset_editor::VegetationPresetEditorState,
    },
};
use bevy::{
    input_focus::InputFocus,
    prelude::*,
    tasks::{IoTaskPool, block_on},
    text::EditableText,
};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

static COUNT: AtomicU64 = AtomicU64::new(0);
pub(super) struct Fixture(pub(super) PathBuf);
impl Fixture {
    pub(super) fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "rpg-asset-search-{}-{}",
            std::process::id(),
            COUNT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    pub(super) fn write(&self, path: &str, text: impl AsRef<[u8]>) {
        let target = self.0.join(path);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(target, text).unwrap();
    }
    pub(super) fn request(&self, query: &str) -> SearchRequest {
        SearchRequest {
            ticket: 1,
            root: self.0.clone(),
            folder: String::new(),
            query: query.into(),
            references: false,
            missing_assets: false,
            filters: Vec::new(),
            case_sensitive: true,
            whole_word: false,
            drafts: Vec::new(),
            cancel: AssetSearchState::cancellation(),
        }
    }
    pub(super) fn doc(&self, path: &str) -> Document {
        let (disk, source) = io::load(&self.0, path).unwrap();
        Document {
            path: path.into(),
            format: crate::asset_catalog::document_format(path).unwrap(),
            generation: 1,
            disk,
            pages: source.pages,
            source: source.text,
            revision: 0,
            saved_revision: 0,
            history: default(),
            mode: ViewMode::Edit,
            preview_scroll: Vec2::ZERO,
            preview: Arc::new(Vec::new()),
            preview_revision: None,
            preview_clipped: false,
            confirm_reload: false,
            external: default(),
            status: String::new(),
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub(super) fn search(request: SearchRequest) -> SearchResult {
    let _app = {
        let mut app = App::new();
    app.insert_resource(feathrise_editor_documents::content_operations());
        app.add_plugins(MinimalPlugins);
        app
    };
    // 同步等待只属于测试线程；生产系统始终使用check_ready。
    block_on(IoTaskPool::get().spawn(worker::run(request, None)))
}

#[test]
fn cross_file_filters_accept_extensions_named_files_and_reject_arbitrary_globs() {
    let filters = worker::filters(" .JSON, *.rs; WGSL, json ").unwrap();
    assert_eq!(filters, ["json", "rs", "wgsl"]);
    assert!(worker::accepts("中文/Test.JSON", &filters));
    assert!(!worker::accepts("image.png", &[]));
    assert!(worker::accepts(
        "folder/README",
        &worker::filters("README").unwrap()
    ));
    assert!(worker::accepts(
        "folder/.gitignore",
        &worker::filters(".gitignore").unwrap()
    ));
    assert!(worker::filters("**/*.json").is_err());
    assert!(worker::filters("../json").is_err());
    assert!(
        worker::filters(
            &(0..17)
                .map(|n| format!("x{n}"))
                .collect::<Vec<_>>()
                .join(",")
        )
        .is_err()
    );
}

#[test]
fn cross_file_scope_uses_directory_boundaries_and_unicode_byte_columns() {
    assert!(worker::in_folder("config/a/b.json", "config"));
    assert!(!worker::in_folder("config_old/b.json", "config"));
    let regex = navigation::literal_regex("长剑", true, false).unwrap();
    let text = "中文 长剑\n\t长剑 长剑";
    let hits = worker::match_text(&regex, text, 10, &std::sync::atomic::AtomicBool::new(false));
    assert_eq!(
        hits.iter()
            .map(|hit| (hit.line, hit.column))
            .collect::<Vec<_>>(),
        [(1, 4), (2, 2), (2, 5)]
    );
    for hit in &hits {
        assert_eq!(&text[hit.range.clone()], "长剑");
        assert!(hit.snippet.contains("‹长剑›"));
    }
}

#[test]
fn cross_file_matching_shares_casefold_whole_word_and_literal_punctuation() {
    let regex = navigation::literal_regex("weapon", false, true).unwrap();
    let hits = worker::match_text(
        &regex,
        "Weapon weapon weapon_extra",
        10,
        &std::sync::atomic::AtomicBool::new(false),
    );
    assert_eq!(hits.len(), 2);
    let regex = navigation::literal_regex("$1\\x", true, false).unwrap();
    assert_eq!(
        worker::match_text(
            &regex,
            "$1\\x $2\\x",
            10,
            &std::sync::atomic::AtomicBool::new(false)
        )
        .len(),
        1
    );
}

#[test]
fn cross_file_worker_recurses_groups_and_preserves_disk_bytes() {
    let fixture = Fixture::new();
    fixture.write("config/剑.JSON", "\u{feff}{\"名称\":\"长剑\"}\r\n长剑");
    fixture.write("scripts/剑.rs", "// 长剑\nfn main() {}\n");
    fixture.write("config_old/a.txt", "长剑");
    fixture.write("binary.png", "长剑");
    let result = search(fixture.request("长剑"));
    assert_eq!(result.files.len(), 3);
    assert_eq!(result.rows.len(), 7);
    assert!(!result.limited);
    assert_eq!(result.files[0].path, "config/剑.JSON");
    assert_eq!(
        (result.files[0].hits[1].line, result.files[0].hits[1].column),
        (2, 1)
    );
    assert_eq!(
        std::fs::read_to_string(fixture.0.join("config/剑.JSON")).unwrap(),
        "\u{feff}{\"名称\":\"长剑\"}\r\n长剑"
    );
    let mut request = fixture.request("长剑");
    request.folder = "config".into();
    request.filters = worker::filters("json").unwrap();
    assert_eq!(search(request).files.len(), 1);
}

#[test]
fn cross_file_worker_uses_unsaved_draft_instead_of_disk_and_checks_read_limits() {
    let fixture = Fixture::new();
    fixture.write("draft.txt", "磁盘旧值");
    fixture.write("bad.txt", [0xff, 0xfe]);
    fixture.write("large.txt", "甲".repeat(MAX_FILE_BYTES));
    fixture.write("nul.txt", "正文\0二进制");
    let doc = fixture.doc("draft.txt");
    let mut request = fixture.request("草稿");
    request.drafts.push(Draft {
        stamp: DraftStamp {
            key: doc.source_key(),
            revision: 4,
        },
        text: Arc::from("未保存草稿"),
        dirty: true,
    });
    let result = search(request);
    assert_eq!(result.files.len(), 1);
    assert!(result.files[0].dirty);
    assert_eq!(result.skipped, 3);
    assert!(matches!(
        result.files[0].identity,
        ContentIdentity::Draft(_)
    ));
    assert_eq!(
        std::fs::read_to_string(fixture.0.join("draft.txt")).unwrap(),
        "磁盘旧值"
    );
}

#[test]
fn cross_file_worker_rejects_outside_scope_and_reports_cancellation_and_result_cap() {
    let fixture = Fixture::new();
    fixture.write("a.txt", "x".repeat(MAX_HITS + 5));
    let result = search(fixture.request("x"));
    assert!(result.limited);
    assert_eq!(result.files[0].hits.len(), MAX_HITS);
    let mut request = fixture.request("x");
    request.folder = "../".into();
    assert!(!search(request).error.is_empty());
    let request = fixture.request("x");
    request.cancel.store(true, Ordering::Relaxed);
    assert!(search(request).cancelled);
}

pub(super) fn control_app(fixture: &Fixture) -> (App, Entity) {
    let mut app = App::new();
    app.insert_resource(feathrise_editor_documents::content_operations());
    app.add_plugins(MinimalPlugins)
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<InputFocus>()
        .init_resource::<AssetSearchState>()
        .init_resource::<MapEditorSidebarState>()
        .init_resource::<MapEditorLayoutState>()
        .init_resource::<EditorAssetCatalog>()
        .init_resource::<EditorTextInputGuard>()
        .init_resource::<MapEditorNumericScrubMouseGuard>()
        .init_resource::<VegetationPresetEditorState>()
        .add_message::<crate::ui::external_files::OpenExternalFile>()
        .insert_resource(crate::ui::external_files::ExternalEditors::for_tests(&[crate::ui::external_files::ExternalEditor::VsCode]))
        .insert_resource(MapEditorState {
            open: true,
            ..default()
        })
        .insert_resource(DocumentState {
            root: fixture.0.clone(),
            ..default()
        })
        .add_systems(Update, (control::input, tasks::drive).chain());
    let mut text = EditableText::default();
    text.editor.set_text("长剑");
    let field = app.world_mut().spawn((SearchField::Query, text)).id();
    (app, field)
}

#[test]
fn external_search_result_opens_real_line_without_changing_selection_and_rejects_stale_or_uninstalled() {
    use crate::ui::external_files::{ExternalEditor, ExternalEditors, OpenExternalFile};
    let fixture = Fixture::new();
    fixture.write("中文 空格.rs", "第一行\n第二行\n第三行长剑\n");
    let result = Arc::new(search(fixture.request("长剑")));
    for (ide, in_project) in ExternalEditor::ALL.into_iter().flat_map(|ide| [false, true].map(|in_project| (ide, in_project))) {
        let (mut app, _) = control_app(&fixture);
        app.world_mut().resource_mut::<MapEditorSidebarState>().navigator = EditorNavigatorTab::Assets;
        app.insert_resource(ExternalEditors::for_tests(&ExternalEditor::ALL));
        {
            let mut state = app.world_mut().resource_mut::<AssetSearchState>();
            state.open = true; state.query = "长剑".into(); state.ticket = 1; state.result = Some(result.clone());
        }
        let choice = app.world_mut().spawn((SearchAction::ChooseEditor(ide), feathrise_editor_ui::EditorButtonClick::triggered(), Interaction::Pressed)).id();
        app.update();
        assert_eq!(app.world().resource::<AssetSearchState>().preferred_editor, Some(ide));
        assert!(app.world().resource::<Messages<OpenExternalFile>>().is_empty());
        *app.world_mut().get_mut::<Interaction>(choice).unwrap() = Interaction::None; app.update();
        let index = app.world().resource::<AssetSearchState>().current().unwrap().rows.iter().position(|row| matches!(row, ResultRow::Hit(..))).unwrap();
        let button = app.world_mut().spawn((SearchAction::External { ticket: 1, index, editor: ide, in_project }, feathrise_editor_ui::EditorButtonClick::triggered(), Interaction::Pressed)).id();
        app.update();
        let requests: Vec<_> = app.world_mut().resource_mut::<Messages<OpenExternalFile>>().drain().collect();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].path, "中文 空格.rs");
        assert_eq!(requests[0].action.editor(), Some(ide));
        assert_eq!(requests[0].action.in_project(), in_project);
        assert_eq!(requests[0].action.location(), Some((3, 1)));
        assert!(app.world().resource::<DocumentState>().expanded.is_none());
        assert!(app.world().resource::<MapEditorState>().selected_asset.is_none());
        *app.world_mut().get_mut::<Interaction>(button).unwrap() = Interaction::None; app.update();
        app.insert_resource(crate::ui::external_files::ExternalEditors::default());
        *app.world_mut().get_mut::<Interaction>(button).unwrap() = Interaction::Pressed;
        app.world_mut().entity_mut(button).insert(feathrise_editor_ui::EditorButtonClick::triggered()); app.update();
        assert!(app.world().resource::<Messages<OpenExternalFile>>().is_empty()); // 隐藏入口的旧按下不能启动未安装工具。
        *app.world_mut().get_mut::<Interaction>(button).unwrap() = Interaction::None; app.update();
        app.insert_resource(ExternalEditors::for_tests(&ExternalEditor::ALL));
        app.world_mut().resource_mut::<AssetSearchState>().ticket += 1;
        *app.world_mut().get_mut::<Interaction>(button).unwrap() = Interaction::Pressed;
        app.world_mut().entity_mut(button).insert(feathrise_editor_ui::EditorButtonClick::triggered()); app.update();
        assert!(app.world().resource::<Messages<OpenExternalFile>>().is_empty());
    }
}

#[test]
fn cross_file_ctrl_shift_f_focuses_panel_and_ime_never_submits() {
    let fixture = Fixture::new();
    let (mut app, field) = control_app(&fixture);
    app.world_mut().resource_mut::<MapEditorLayoutState>().content_visible = false;
    app.world_mut().resource_mut::<MapEditorSidebarState>().right = EditorRightSidebarTab::Tools;
    {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.press(KeyCode::ControlLeft);
        keys.press(KeyCode::ShiftLeft);
        keys.press(KeyCode::KeyF);
    }
    app.update();
    assert!(app.world().resource::<AssetSearchState>().open);
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(field));
    assert_eq!(
        app.world().resource::<MapEditorSidebarState>().right,
        EditorRightSidebarTab::Tools
    );
    let sidebar = app.world().resource::<MapEditorSidebarState>();
    assert_eq!(sidebar.navigator, EditorNavigatorTab::Assets);
    assert_eq!(sidebar.inspector, EditorInspectorTarget::Scene);
    assert!(app.world().resource::<MapEditorLayoutState>().content_visible);
    assert!(
        !app.world()
            .resource::<ButtonInput<KeyCode>>()
            .just_pressed(KeyCode::KeyF)
    );
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .clear();
    app.world_mut()
        .resource_mut::<EditorTextInputGuard>()
        .block_shortcuts = true;
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Enter);
    app.update();
    assert!(app.world().resource::<AssetSearchState>().pending.is_none());
    assert!(app.world().resource::<AssetSearchState>().task.is_none());
}

#[test]
fn cross_file_cancel_retains_old_slot_until_completion_and_runs_latest_request() {
    let fixture = Fixture::new();
    fixture.write("a.txt", "长剑");
    let (mut app, _) = control_app(&fixture);
    let (release, receiver) = std::sync::mpsc::channel();
    {
        let mut state = app.world_mut().resource_mut::<AssetSearchState>();
        state.open = true;
        state.ticket = 1;
        state.running = Some(fixture.request("长剑"));
        state.task = Some(IoTaskPool::get().spawn(async move {
            receiver.recv().unwrap();
            SearchResult {
                ticket: 1,
                ..default()
            }
        }));
        state.invalidate("取消旧任务");
        let mut next = fixture.request("长剑");
        next.ticket = state.ticket;
        state.pending = Some(next);
    }
    app.update();
    assert!(app.world().resource::<AssetSearchState>().task.is_some());
    assert!(app.world().resource::<AssetSearchState>().pending.is_some());
    release.send(()).unwrap();
    for _ in 0..2000 {
        app.update();
        if app
            .world()
            .resource::<AssetSearchState>()
            .current()
            .is_some()
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let state = app.world().resource::<AssetSearchState>();
    assert_eq!(state.current().unwrap().files.len(), 1);
    assert_eq!(state.current().unwrap().ticket, 2);
}

#[test]
fn cross_file_jump_identity_refuses_external_changes_and_edited_drafts() {
    let fixture = Fixture::new();
    fixture.write("a.txt", "长剑");
    let mut doc = fixture.doc("a.txt");
    let disk = ContentIdentity::Disk(doc.disk.digest);
    let draft = ContentIdentity::Draft(DraftStamp {
        key: doc.source_key(),
        revision: doc.revision,
    });
    assert!(jump::identity_matches(&disk, &doc));
    assert!(jump::identity_matches(&draft, &doc));
    doc.edit("草稿长剑");
    assert!(!jump::identity_matches(&disk, &doc));
    assert!(!jump::identity_matches(&draft, &doc));
    std::fs::write(fixture.0.join("a.txt"), "外部改变").unwrap();
    let next = fixture.doc("a.txt");
    assert!(!jump::identity_matches(&disk, &next));
}

#[test]
fn cross_file_editor_close_keeps_cancelled_slot_and_resume_requires_new_search() {
    let fixture = Fixture::new();
    let (mut app, _) = control_app(&fixture);
    let (release, receiver) = std::sync::mpsc::channel();
    let request = fixture.request("长剑");
    let cancel = request.cancel.clone();
    {
        let mut state = app.world_mut().resource_mut::<AssetSearchState>();
        state.open = true;
        state.ticket = 1;
        state.running = Some(request.clone());
        state.pending = Some(request);
        state.task = Some(IoTaskPool::get().spawn(async move {
            receiver.recv().unwrap();
            SearchResult {
                ticket: 1,
                ..default()
            }
        }));
    }
    app.world_mut().resource_mut::<MapEditorState>().open = false;
    app.update();
    assert!(cancel.load(Ordering::Relaxed));
    assert!(app.world().resource::<AssetSearchState>().task.is_some());
    assert!(app.world().resource::<AssetSearchState>().pending.is_none());
    release.send(()).unwrap();
    for _ in 0..1000 {
        app.update();
        if app.world().resource::<AssetSearchState>().task.is_none() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    app.world_mut().resource_mut::<MapEditorState>().open = true;
    app.update();
    let state = app.world().resource::<AssetSearchState>();
    assert!(state.task.is_none() && state.pending.is_none() && state.current().is_none());
}
