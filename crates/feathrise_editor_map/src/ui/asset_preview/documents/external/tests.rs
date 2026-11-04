//! 真实临时磁盘修改、焦点边沿、草稿冲突及迟到核对结果回归；不改正式资产。

use super::{systems, types::*};
use super::super::{io, resources::DocumentState, session, types::*};
use bevy::{prelude::*, tasks::IoTaskPool, window::PrimaryWindow};
use std::{path::PathBuf, sync::{Arc, atomic::{AtomicU64, Ordering}}};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        IoTaskPool::get_or_init(bevy::tasks::TaskPool::new);
        let root = std::env::temp_dir().join(format!("rpg-document-external-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("说明.md"), "原文\r\n").unwrap();
        Self(root)
    }
    fn write(&self, text: impl AsRef<[u8]>) { std::fs::write(self.0.join("说明.md"), text).unwrap(); }
    fn state(&self) -> DocumentState {
        let (disk, contents) = io::load(&self.0, "说明.md").unwrap();
        DocumentState {
            root: self.0.clone(), active: Some("说明.md".into()), load_ticket: 1,
            documents: vec![Document {
                path: "说明.md".into(), format: crate::asset_catalog::DocumentFormat::Markdown,
                generation: 1, disk, pages: contents.pages, source: contents.text,
                revision: 0, saved_revision: 0, history: default(), preview: Arc::new(Vec::new()),
                preview_revision: None, preview_clipped: false, mode: ViewMode::Edit,
                preview_scroll: Vec2::new(0.0, 73.0), confirm_reload: false, external: default(), status: String::new(),
            }], ..default()
        }
    }
}
impl Drop for Fixture { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }

fn check(state: &DocumentState) -> CheckReply {
    systems::check(state.root.clone(), state.documents.iter().map(|doc| CheckSnapshot {
        key: doc.source_key(), revision: doc.revision, disk: doc.disk.clone(),
    }).collect())
}

#[test]
fn external_clean_document_refreshes_full_content_bom_and_generation_preserving_view_mode() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    let old = state.current().unwrap().source_key();
    fixture.write("\u{feff}外部新文\r\n第二行\n");
    let reply = check(&state);
    systems::adopt(&mut state, reply);
    let doc = state.current().unwrap();
    assert_eq!(doc.source_text(), "外部新文\n第二行\n");
    assert!(doc.disk.bom && doc.disk.crlf && !doc.dirty());
    assert_ne!(doc.source_key(), old);
    assert_eq!(doc.mode, ViewMode::Edit);
    assert_eq!(doc.preview_scroll.y, 73.0);
    assert!(!doc.external.warning);
}

#[test]
fn external_dirty_document_keeps_draft_and_save_baseline_even_after_notice_is_dismissed() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    state.current_mut().unwrap().edit("内置草稿");
    let baseline = state.current().unwrap().disk.clone();
    fixture.write("外部修订一");
    let reply = check(&state); systems::adopt(&mut state, reply);
    assert!(state.current().unwrap().external.warning);
    assert_eq!(state.current().unwrap().source_text(), "内置草稿");
    assert_eq!(state.current().unwrap().disk, baseline);
    state.current_mut().unwrap().external.warning = false;
    let reply = check(&state); systems::adopt(&mut state, reply);
    assert!(!state.current().unwrap().external.warning);
    let doc = state.current().unwrap();
    assert!(io::save(&fixture.0, &doc.path, &doc.disk, &doc.pages).is_err());
    assert_eq!(std::fs::read_to_string(fixture.0.join("说明.md")).unwrap(), "外部修订一");
    fixture.write("外部修订二");
    let reply = check(&state); systems::adopt(&mut state, reply);
    assert!(state.current().unwrap().external.warning);
    assert_eq!(state.current().unwrap().source_text(), "内置草稿");
}

#[test]
fn external_check_during_edit_or_undo_is_requeued_without_overwriting_new_input() {
    let fixture = Fixture::new(); let mut state = fixture.state();
    fixture.write("外部文本"); let reply = check(&state);
    state.current_mut().unwrap().edit("核对期间新输入");
    systems::adopt(&mut state, reply);
    assert_eq!(state.current().unwrap().source_text(), "核对期间新输入");
    assert!(!state.current().unwrap().external.warning);
    assert!(state.external.requested);
    let reply = check(&state); systems::adopt(&mut state, reply);
    assert!(state.current().unwrap().external.warning);
}

#[test]
fn external_check_rejects_old_generation_changed_root_and_changed_save_baseline() {
    let fixture = Fixture::new(); let mut state = fixture.state();
    fixture.write("外部文本"); let reply = check(&state);
    state.current_mut().unwrap().generation += 1;
    systems::adopt(&mut state, reply);
    assert_eq!(state.current().unwrap().source_text(), "原文\n");
    let mut reply = check(&state); reply.root = fixture.0.join("other");
    systems::adopt(&mut state, reply);
    assert_eq!(state.current().unwrap().source_text(), "原文\n");
    let reply = check(&state);
    state.current_mut().unwrap().disk.digest = [9; 32];
    systems::adopt(&mut state, reply);
    assert_eq!(state.current().unwrap().source_text(), "原文\n");
}

#[test]
fn external_refresh_preserves_newly_queued_selection_load_and_uses_a_fresh_source_key() {
    let fixture = Fixture::new(); let mut state = fixture.state();
    state.load_ticket = 0; // 缓存/测试会话代次也必须比刷新票据旧。
    fixture.write("更新"); let reply = check(&state);
    state.pending_load = Some(LoadRequest { path: "下一篇.md".into(), ticket: 2 });
    systems::adopt(&mut state, reply);
    assert!(state.current().unwrap().generation > 1);
    assert_eq!(state.pending_load.as_ref().unwrap().path, "下一篇.md");
    assert_eq!(state.pending_load.as_ref().unwrap().ticket, state.load_ticket);
}

#[test]
fn external_deleted_invalid_utf8_and_oversized_documents_preserve_original_content() {
    let fixture = Fixture::new(); let mut state = fixture.state();
    std::fs::remove_file(fixture.0.join("说明.md")).unwrap();
    let reply = check(&state); systems::adopt(&mut state, reply);
    assert!(state.current().unwrap().external.warning);
    for bytes in [vec![0xff], vec![b'x'; super::super::constants::MAX_FILE_BYTES + 1]] {
        fixture.write(bytes); let reply = check(&state); systems::adopt(&mut state, reply);
        assert!(state.current().unwrap().external.warning);
        assert_eq!(state.current().unwrap().source_text(), "原文\n");
    }
}

#[test]
fn external_ready_check_waits_for_native_input_and_window_then_adopts_once() {
    let fixture = Fixture::new(); let mut state = fixture.state();
    fixture.write("更新"); state.external.ready = Some(check(&state));
    systems::drive(&mut state, false); // 失焦时不重绑隐藏正文。
    assert!(state.external.ready.is_some());
    state.external.available = true;
    systems::drive(&mut state, true); // IME、粘贴和待采用编辑先完成。
    assert_eq!(state.current().unwrap().source_text(), "原文\n");
    systems::drive(&mut state, false);
    assert_eq!(state.current().unwrap().source_text(), "更新");
    assert!(state.external.ready.is_none());
}

#[test]
fn external_check_keeps_single_io_slot_and_explicit_load_priority() {
    let fixture = Fixture::new(); let mut state = fixture.state();
    state.external.available = true; state.external.requested = true;
    state.io = Some(IoTaskPool::get().spawn(std::future::pending()));
    systems::drive(&mut state, false);
    assert!(state.external.requested && state.io.is_some());
    state.io = None;
    state.pending_load = Some(LoadRequest { path: "下一篇.md".into(), ticket: 2 });
    systems::drive(&mut state, false);
    assert!(state.external.requested && state.io.is_none());
    state.pending_load = None;
    systems::drive(&mut state, false);
    assert!(!state.external.requested && state.io.is_some());
}

#[test]
fn external_ready_check_does_not_invalidate_an_inflight_load_and_retickets_a_queued_load() {
    let fixture = Fixture::new(); let mut state = fixture.state();
    fixture.write("旧文件的外部更新"); state.external.ready = Some(check(&state));
    state.external.available = true;
    state.io = Some(IoTaskPool::get().spawn(std::future::pending()));
    systems::drive(&mut state, false);
    assert!(state.external.ready.is_some());
    assert_eq!(state.current().unwrap().source_text(), "原文\n");
    state.io = None;
    std::fs::write(fixture.0.join("下一篇.md"), "所选新文件").unwrap();
    state.active = Some("下一篇.md".into()); state.request_load("下一篇.md".into());
    let mut app = App::new();
    app.insert_resource(feathrise_editor_documents::content_operations());
    app.add_plugins(MinimalPlugins).insert_resource(state).add_systems(Update, session::drive_tasks);
    settle(&mut app);
    assert_eq!(app.world().resource::<DocumentState>().current().unwrap().source_text(), "所选新文件");
    assert_eq!(app.world().resource::<DocumentState>().documents.iter().find(|doc| doc.path == "说明.md").unwrap().source_text(), "旧文件的外部更新");
}

#[test]
fn external_focus_and_f12_edges_refresh_but_stable_frames_do_not_poll_disk() {
    let fixture = Fixture::new();
    let mut app = App::new();
    app.insert_resource(feathrise_editor_documents::content_operations());
    app.add_plugins(MinimalPlugins).insert_resource(fixture.state())
        .insert_resource(crate::resources::MapEditorState { open: true, ..default() })
        .add_systems(Update, (systems::request_check, session::drive_tasks).chain());
    let window = app.world_mut().spawn((Window { focused: true, ..default() }, PrimaryWindow)).id();
    settle(&mut app);
    fixture.write("首次返回更新"); settle(&mut app);
    assert_eq!(app.world().resource::<DocumentState>().current().unwrap().source_text(), "原文\n");
    app.world_mut().get_mut::<Window>(window).unwrap().focused = false; app.update();
    app.world_mut().get_mut::<Window>(window).unwrap().focused = true; settle(&mut app);
    assert_eq!(app.world().resource::<DocumentState>().current().unwrap().source_text(), "首次返回更新");
    app.world_mut().resource_mut::<crate::resources::MapEditorState>().open = false; app.update();
    fixture.write("F12重新打开更新");
    app.world_mut().resource_mut::<crate::resources::MapEditorState>().open = true; settle(&mut app);
    assert_eq!(app.world().resource::<DocumentState>().current().unwrap().source_text(), "F12重新打开更新");
}

fn settle(app: &mut App) {
    for _ in 0..1000 {
        app.update();
        let state = app.world().resource::<DocumentState>();
        if state.io.is_none() && state.external.ready.is_none() && !state.external.requested { return; }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    panic!("外部核对未在1秒内结清");
}
