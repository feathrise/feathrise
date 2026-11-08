//! 重载只冻结当前真实读取目标；原生输入、保存和外部核对共用原系统，不伪造任务结果。

use super::*;
use crate::resources::{EditorTextInputGuard, MapEditorNumericScrubMouseGuard, MapEditorState};
use crate::ui::{
    asset_preview::types::{AssetPreviewContent, AssetPreviewState},
    sidebars::{EditorRightSidebarTab, MapEditorSidebarState},
    vegetation_preset_editor::VegetationPresetEditorState,
};
use bevy::{ecs::system::RunSystemOnce, input_focus::InputFocus,
    text::{FontCx, LayoutCx, TextEdit}};

/// 只暂缓主线程轮询真实IO，稳定制造“任务已启动但回执未采用”的交互窗口。
fn native_app(fixture: &Fixture, document: Document) -> (App, Entity) {
    let path = document.path.clone();
    let generation = document.generation;
    let mut sidebar = MapEditorSidebarState::default();
    sidebar.right = EditorRightSidebarTab::Assets;
    let mut font = FontCx::default();
    let embedded = Font::from_bytes(feathrise_runtime::app::default_font::DEFAULT_FONT_BYTES.to_vec());
    font.context.collection.register_fonts(embedded.data, None);
    font.set_sans_serif_family("Noto Sans CJK SC").unwrap();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(feathrise_editor_documents::content_operations())
        .insert_resource(font).init_resource::<LayoutCx>()
        .init_resource::<bevy::clipboard::Clipboard>()
        .init_resource::<InputFocus>().init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<EditorTextInputGuard>().init_resource::<MapEditorNumericScrubMouseGuard>()
        .init_resource::<super::super::resources::DocumentInputs>()
        .init_resource::<VegetationPresetEditorState>()
        .insert_resource(MapEditorState { open: true, selected_asset: Some(path.clone()), ..default() })
        .insert_resource(sidebar)
        .insert_resource(AssetPreviewState { selected_path: Some(path.clone()), content: AssetPreviewContent::Document, ..default() })
        .insert_resource(DocumentState {
            root: fixture.root.clone(), project_root: None, active: Some(path), visible: true,
            load_ticket: generation, documents: vec![document], ..default()
        })
        .add_systems(Update, (bevy::text::apply_text_edits, session::capture_edits, super::super::native::bind).chain());
    let source = app.world_mut().spawn((DocumentSource::default(), EditableText::default(),
ComputedNode::default())).id();
    app.update();
    app.update(); // 原生绑定排队的TextStart先由官方输入系统采用。
    (app, source)
}

fn press(app: &mut App, action: DocumentAction) {
    let button = app.world_mut().spawn((action, feathrise_editor_ui::EditorButtonClick::triggered(), Interaction::Pressed)).id();
    app.world_mut().run_system_once(super::super::interaction::actions).unwrap();
    app.world_mut().despawn(button);
}

fn replace(app: &mut App, source: Entity, text: &str) {
    let mut input = app.world_mut().get_mut::<EditableText>(source).unwrap();
    input.queue_edit(TextEdit::SelectAll);
    input.queue_edit(TextEdit::Insert(text.into()));
    app.update();
}

fn begin_reload(app: &mut App) {
    press(app, DocumentAction::Reload);
    if app.world().resource::<DocumentState>().pending_load.is_none() {
        assert!(app.world().resource::<DocumentState>().current().unwrap().confirm_reload);
        press(app, DocumentAction::Reload);
    }
    assert!(app.world().resource::<DocumentState>().pending_load.is_some());
    app.world_mut().run_system_once(session::drive_tasks).unwrap();
    assert!(app.world().resource::<DocumentState>().io.is_some());
    assert!(app.world().resource::<DocumentState>().loading.is_some());
    app.update();
}

fn settle_real_io(app: &mut App) {
    let started = std::time::Instant::now();
    loop {
        app.world_mut().run_system_once(session::drive_tasks).unwrap();
        app.update();
        let state = app.world().resource::<DocumentState>();
        if state.io.is_none() && state.loading.is_none() && state.pending_load.is_none() && state.parser.is_none()
            && state.external.ready.is_none() && !state.external.requested { return; }
        assert!(started.elapsed() < std::time::Duration::from_secs(5), "真实文档任务未在5秒内结清");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

#[test]
fn markdown_reload_failure_keeps_original_session_history_and_resumes_native_editing() {
    let fixture = Fixture::new();
    fixture.write("磁盘基线\n");
    let mut doc = fixture.document();
    doc.edit("失败时必须保留的草稿\n");
    let key = doc.source_key();
    let disk = doc.disk.clone();
    let revision = doc.revision;
    assert!(doc.history.can_undo());
    let (mut app, source) = native_app(&fixture, doc);
    fixture.write([0xff, 0xfe]); // 正式受检读取必须返回非法UTF-8，不能将失败伪造为成功回执。
    begin_reload(&mut app);
    press(&mut app, DocumentAction::Edit);
    app.update();
    let during = app.world().get::<DocumentSource>(source).unwrap().key.clone();
    settle_real_io(&mut app);
    {
        let state = app.world().resource::<DocumentState>();
        let doc = state.current().unwrap();
        assert!(state.loading.is_none());
        assert!(!state.error.is_empty());
        assert_eq!(doc.source_key(), key);
        assert_eq!(doc.disk, disk);
        assert_eq!(doc.revision, revision);
        assert!(doc.history.can_undo());
        assert_eq!(doc.source_text(), "失败时必须保留的草稿\n");
        assert!(doc.dirty());
    }
    {
        let mut state = app.world_mut().resource_mut::<DocumentState>();
        let doc = state.current_mut().unwrap();
        doc.travel(false).unwrap();
        assert_eq!(doc.source_text(), "磁盘基线\n");
        doc.travel(true).unwrap();
        assert_eq!(doc.source_text(), "失败时必须保留的草稿\n");
    }
    press(&mut app, DocumentAction::Edit);
    app.update(); app.update();
    assert_eq!(app.world().get::<EditableText>(source).unwrap().value(), "失败时必须保留的草稿\n");
    replace(&mut app, source, "读取失败后仍能通过官方输入编辑\n");
    assert_eq!(app.world().resource::<DocumentState>().current().unwrap().source_text(), "读取失败后仍能通过官方输入编辑\n");
    assert_eq!(during, None);
    assert_eq!(std::fs::read(fixture.root.join("说明.md")).unwrap(), [0xff, 0xfe]);
}

#[test]
fn markdown_reload_pending_and_inflight_reject_residual_native_input_even_if_other_entry_sets_edit() {
    for inflight in [false, true] {
        let fixture = Fixture::new(); fixture.write("原磁盘正文\n");
        let mut doc = fixture.document(); doc.edit("确认前的草稿\n");
        let key = doc.source_key();
        let revision = doc.revision;
        let (mut app, source) = native_app(&fixture, doc);
        fixture.write([0xff]);
        app.world_mut().resource_mut::<DocumentState>().request_load("说明.md".into());
        if inflight { app.world_mut().run_system_once(session::drive_tasks).unwrap(); }
        // 模拟任意遗漏入口保持Edit及尚未解绑的旧控件；保护必须落在共享输入边界。
        replace(&mut app, source, "不能进入正文或失败后缓存的残余输入\n");
        let frozen = app.world().resource::<DocumentState>().current().unwrap().source_text().to_owned();
        let binding = app.world().get::<DocumentSource>(source).unwrap().key.clone();
        settle_real_io(&mut app);
        app.update(); app.update();
        let doc = app.world().resource::<DocumentState>().current().unwrap();
        assert_eq!(frozen, "确认前的草稿\n");
        assert_eq!(binding, None);
        assert_eq!(doc.source_text(), "确认前的草稿\n", "读取失败后也不能从原生缓存延迟回灌被拒绝的文字");
        assert_eq!(doc.source_key(), key);
        assert_eq!(doc.revision, revision);
        assert_eq!(app.world().get::<EditableText>(source).unwrap().value(), doc.source_text());
    }
}

#[test]
fn markdown_reload_receipt_rejects_confirmed_revision_changed_by_business_history() {
    let fixture = Fixture::new(); fixture.write("磁盘旧正文\n");
    let mut doc = fixture.document(); doc.edit("确认丢弃时的草稿\n");
    let key = doc.source_key();
    let revision = doc.revision;
    let disk = doc.disk.clone();
    let (mut app, _) = native_app(&fixture, doc);
    fixture.write("重新读取到的真实磁盘正文\n");
    begin_reload(&mut app);
    {
        let mut state = app.world_mut().resource_mut::<DocumentState>();
        let doc = state.current_mut().unwrap();
        doc.edit("其他业务入口的新修改\n");
        doc.travel(false).unwrap(); // 文字回到确认时的草稿，单调修订仍变化，不能只比正文或dirty。
        assert_eq!(doc.source_text(), "确认丢弃时的草稿\n");
    }
    settle_real_io(&mut app);
    let state = app.world().resource::<DocumentState>();
    let doc = state.current().unwrap();
    assert!(state.loading.is_none());
    assert!(state.error.contains("已取消载入"));
    assert_eq!(doc.source_key(), key);
    assert_eq!(doc.disk, disk);
    assert!(doc.revision > revision && doc.history.can_redo());
    assert_eq!(doc.source_text(), "确认丢弃时的草稿\n");
    assert_eq!(std::fs::read_to_string(fixture.root.join("说明.md")).unwrap(), "重新读取到的真实磁盘正文\n");
}

#[test]
fn markdown_reload_guard_allows_official_native_edits_during_real_save() {
    let fixture = Fixture::new(); fixture.write("保存前磁盘正文\n");
    let (mut app, source) = native_app(&fixture, fixture.document());
    replace(&mut app, source, "明确提交保存的正文\n");
    press(&mut app, DocumentAction::Save);
    assert!(app.world().resource::<DocumentState>().io.is_some());
    assert!(app.world().resource::<DocumentState>().loading.is_none());
    replace(&mut app, source, "保存回执采用前继续输入的草稿\n");
    let bound = app.world().get::<DocumentSource>(source).unwrap().key.clone();
    let edited = app.world().resource::<DocumentState>().current().unwrap().source_text().to_owned();
    settle_real_io(&mut app);
    let doc = app.world().resource::<DocumentState>().current().unwrap();
    assert_eq!(bound, Some(doc.source_key()));
    assert_eq!(edited, "保存回执采用前继续输入的草稿\n");
    assert_eq!(doc.source_text(), edited);
    assert!(doc.dirty());
    assert_eq!(std::fs::read_to_string(fixture.root.join("说明.md")).unwrap(), "明确提交保存的正文\n");
}

#[test]
fn markdown_reload_guard_allows_official_native_edits_during_real_external_check() {
    let fixture = Fixture::new(); fixture.write("核对前磁盘正文\n");
    let (mut app, source) = native_app(&fixture, fixture.document());
    let key = app.world().resource::<DocumentState>().current().unwrap().source_key();
    fixture.write("外部修改的真实磁盘正文\n");
    {
        let mut state = app.world_mut().resource_mut::<DocumentState>();
        state.external.available = true; state.external.requested = true;
    }
    app.world_mut().run_system_once(session::drive_tasks).unwrap();
    assert!(app.world().resource::<DocumentState>().io.is_some());
    assert!(app.world().resource::<DocumentState>().loading.is_none());
    replace(&mut app, source, "外部核对回执前继续编辑的草稿\n");
    let bound = app.world().get::<DocumentSource>(source).unwrap().key.clone();
    let edited = app.world().resource::<DocumentState>().current().unwrap().source_text().to_owned();
    settle_real_io(&mut app);
    let doc = app.world().resource::<DocumentState>().current().unwrap();
    assert_eq!(bound, Some(key.clone()));
    assert_eq!(edited, "外部核对回执前继续编辑的草稿\n");
    assert_eq!(doc.source_key(), key);
    assert_eq!(doc.source_text(), edited);
    assert!(doc.dirty() && doc.external.warning);
    assert_eq!(std::fs::read_to_string(fixture.root.join("说明.md")).unwrap(), "外部修改的真实磁盘正文\n");
}

#[test]
fn markdown_reload_guard_keeps_other_document_editable_and_discards_stale_load_after_tab_switch() {
    let fixture = Fixture::new(); fixture.write("甲的原磁盘正文\n");
    std::fs::write(fixture.root.join("乙.rs"), "// 乙的原磁盘正文\n").unwrap();
    let mut first = fixture.document(); first.edit("甲确认前的草稿\n");
    let second = fixture.document_at("乙.rs");
    let second_key = second.source_key();
    let (mut app, source) = native_app(&fixture, first);
    app.world_mut().resource_mut::<DocumentState>().documents.push(second);
    press(&mut app, DocumentAction::ToggleExpand);
    app.world_mut().run_system_once(session::sync_selection).unwrap();
    fixture.write("甲重新读取的磁盘正文\n");
    begin_reload(&mut app);
    app.world_mut().resource_mut::<MapEditorState>().selected_asset = Some("乙.rs".into());
    press(&mut app, DocumentAction::OpenSelected);
    app.world_mut().run_system_once(session::sync_selection).unwrap();
    app.update(); app.update();
    assert!(app.world().resource::<DocumentState>().io.is_some());
    replace(&mut app, source, "// 甲载入期间乙仍能编辑\n");
    let bound = app.world().get::<DocumentSource>(source).unwrap().key.clone();
    settle_real_io(&mut app);
    let state = app.world().resource::<DocumentState>();
    assert_eq!(bound, Some(second_key));
    assert_eq!(state.current().unwrap().source_text(), "// 甲载入期间乙仍能编辑\n");
    assert!(state.current().unwrap().dirty());
    assert_eq!(state.documents.iter().find(|doc| doc.path == "说明.md").unwrap().source_text(), "甲确认前的草稿\n");
    assert_eq!(std::fs::read_to_string(fixture.root.join("乙.rs")).unwrap(), "// 乙的原磁盘正文\n");
    assert_eq!(std::fs::read_to_string(fixture.root.join("说明.md")).unwrap(), "甲重新读取的磁盘正文\n");
}

#[test]
fn markdown_reload_diagnostic_intent_waits_for_loaded_source_before_selecting_real_new_character() {
    use crate::{asset_catalog::project_files::EditorProjectFileCatalog,
        standalone::OpenProjectSourceLocation, workbench::WorkbenchModel};
    let fixture = Fixture::new();
    std::fs::create_dir(fixture.root.join("assets")).unwrap();
    std::fs::create_dir(fixture.root.join("src")).unwrap();
    let path = fixture.root.join("src/main.rs");
    std::fs::write(&path, "fn main() {\n    let 旧值 = 1;\n}\n").unwrap();
    let mut doc = fixture.document_at("src/main.rs");
    doc.path = "project://src/main.rs".into();
    doc.edit("fn main() {\n    let 草稿 = 0;\n}\n");
    let old_key = doc.source_key();
    let (mut app, source) = native_app(&fixture, doc);
    {
        let mut state = app.world_mut().resource_mut::<DocumentState>();
        state.root = fixture.root.join("assets"); state.project_root = Some(fixture.root.clone());
    }
    press(&mut app, DocumentAction::ToggleExpand);
    app.world_mut().run_system_once(session::sync_selection).unwrap();
    std::fs::write(&path, "fn main() {\n    let 新值 = 2;\n}\n").unwrap();
    begin_reload(&mut app);
    let mut catalog = EditorProjectFileCatalog::default();
    catalog.bind_project_source(0, Some(fixture.root.clone()));
    app.insert_resource(catalog).insert_resource(WorkbenchModel {
        context: 0, project_root: Some(fixture.root.clone()), asset_root: Some(fixture.root.join("assets")), ..default()
    });
    super::super::source_location::install(&mut app);
    app.world_mut().write_message(OpenProjectSourceLocation {
        context: 0, project_root: fixture.root.clone(), path: "src/main.rs".into(), line: 2, column: 9,
    });
    for _ in 0..4 { app.update(); }
    let waiting_mode = app.world().resource::<DocumentState>().current().unwrap().mode;
    let waiting_binding = app.world().get::<DocumentSource>(source).unwrap().key.clone();
    settle_real_io(&mut app);
    let started = std::time::Instant::now();
    loop {
        app.update();
        if app.world().resource::<DocumentState>().current().unwrap().status.contains("已定位编译诊断") { break; }
        assert!(started.elapsed() < std::time::Duration::from_secs(5), "诊断意图不能固定旧代次后被重载吞掉");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let state = app.world().resource::<DocumentState>();
    let doc = state.current().unwrap();
    let input = app.world().get::<EditableText>(source).unwrap();
    let native_text = input.value().to_string();
    assert_eq!(waiting_mode, ViewMode::Preview);
    assert_eq!(waiting_binding, None);
    assert_ne!(doc.source_key(), old_key);
    assert_eq!(app.world().get::<DocumentSource>(source).unwrap().key, Some(doc.source_key()));
    assert_eq!(&native_text[input.editor.raw_selection().text_range()], "新");
    assert_eq!(doc.source_text(), "fn main() {\n    let 新值 = 2;\n}\n");
    assert!(!doc.dirty());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), doc.source_text());
}
