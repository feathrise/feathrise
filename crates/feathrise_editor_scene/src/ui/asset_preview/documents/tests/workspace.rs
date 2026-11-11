//! 中央文档的路径锁定、完整草稿、隐藏输入、显式替换及容量保护回归；文件仅写临时夹具。

use feathrise_editor_ui::UiInteraction;
use super::*;
use crate::resources::{
    EditorTextInputGuard, SceneEditorLayoutState, SceneEditorNumericScrubMouseGuard, SceneEditorState,
};
use crate::ui::asset_preview::{
    AssetPreviewFocus,
    types::{AssetPreviewContent, AssetPreviewState},
};
use crate::ui::sidebars::{EditorRightSidebarTab, SceneEditorSidebarState};
use crate::ui::vegetation_preset_editor::{PresetWorkspaceTab, VegetationPresetEditorState};
use bevy::{ecs::system::RunSystemOnce, input_focus::InputFocus};

fn workspace_app(fixture: &Fixture) -> App {
    let mut app = app(fixture);
    let mut sidebar = SceneEditorSidebarState::default();
    sidebar.right = EditorRightSidebarTab::Assets;
    app.insert_resource(SceneEditorState {
        open: true,
        selected_asset: Some("说明.md".into()),
        ..default()
    })
    .insert_resource(sidebar)
    .insert_resource(AssetPreviewState {
        selected_path: Some("说明.md".into()),
        content: AssetPreviewContent::Document,
        ..default()
    })
    .init_resource::<VegetationPresetEditorState>()
    .init_resource::<SceneEditorLayoutState>()
    .init_resource::<EditorTextInputGuard>()
    .init_resource::<SceneEditorNumericScrubMouseGuard>()
    .init_resource::<ButtonInput<KeyCode>>()
    .init_resource::<InputFocus>()
    .init_resource::<AssetPreviewFocus>();
    app.update();
    app
}

#[test]
fn close_all_document_tabs_retains_each_dirty_draft_and_never_writes_files() {
    let fixture = Fixture::new();
    fixture.write("# 第一文档\n");
    std::fs::write(fixture.root.join("第二.rs"), "fn main() {}\n").unwrap();
    let mut app = workspace_app(&fixture);
    let mut documents = app.world_mut().remove_resource::<DocumentState>().unwrap();
    let mut workspace = app.world_mut().resource_mut::<VegetationPresetEditorState>();
    documents.expand("说明.md".into(), &mut workspace).unwrap();
    documents.documents[0].edit("# 第一份未保存草稿\n");
    let mut second = fixture.document_at("第二.rs");
    second.edit("// 第二份未保存草稿\nfn main() {}\n");
    documents.documents.push(second);
    documents.expand("第二.rs".into(), &mut workspace).unwrap();
    assert_eq!(documents.tab_count(), 2);
    let returned = documents.close_all_tabs().unwrap();
    assert_eq!(returned.0, "第二.rs");
    assert!(!documents.expanded());
    assert_eq!(documents.documents.len(), 2);
    assert!(documents.documents.iter().all(|doc| doc.dirty()));
    assert!(documents.exit_blocker().is_some(), "关闭所有中央标签后，原生窗口退出仍必须保留脏正文");
    assert_eq!(documents.documents[0].source_text(), "# 第一份未保存草稿\n");
    assert_eq!(documents.documents[1].source_text(), "// 第二份未保存草稿\nfn main() {}\n");
    assert_eq!(std::fs::read_to_string(fixture.root.join("说明.md")).unwrap(), "# 第一文档\n");
    assert_eq!(std::fs::read_to_string(fixture.root.join("第二.rs")).unwrap(), "fn main() {}\n");
    documents.expand("说明.md".into(), &mut workspace).unwrap();
    assert!(documents.documents[0].dirty()); // 重新打开同文件仍采用原草稿。
}

#[test]
fn close_all_document_tabs_keeps_the_existing_save_task_and_receipt() {
    let fixture = Fixture::new(); fixture.write("# 原文\n");
    let mut app = workspace_app(&fixture);
    app.world_mut().resource_mut::<DocumentState>().current_mut().unwrap().edit("# 显式保存\n");
    app.world_mut().resource_mut::<DocumentState>().request_save();
    assert!(app.world().resource::<DocumentState>().io.is_some());
    app.world_mut().resource_mut::<DocumentState>().close_all_tabs();
    assert!(app.world().resource::<DocumentState>().io.is_some());
    settle(&mut app);
    assert_eq!(std::fs::read_to_string(fixture.root.join("说明.md")).unwrap(), "# 显式保存\n");
    assert!(!app.world().resource::<DocumentState>().current().unwrap().dirty());
}

fn press(app: &mut App, action: DocumentAction) {
    let entity = app.world_mut().spawn((feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed, action)).id();
    app.world_mut()
        .run_system_once(super::super::interaction::actions)
        .unwrap();
    app.world_mut().despawn(entity);
    sync_selection(app);
}

#[test] // 文档与图片/模型各自固定文件，互相切换或关闭后台媒体页不能留下失效返回标签。
fn asset_document_can_open_from_media_center_and_restore_its_tab() {
    let fixture = Fixture::new();
    std::fs::write(fixture.root.join("说明.md"), "# 文档和模型可以分别放大\n").unwrap();
    let mut app = workspace_app(&fixture);
    app.insert_resource(crate::ui::asset_preview::AssetPreviewWorkspace {
        expanded: Some(crate::ui::asset_preview::types::ExpandedAssetPreview {
            id: 1,
            path: "gltf/tree.glb".into(), return_tab: PresetWorkspaceTab::Scene,
            view: default(), target: default(), restore_cursor: 0, audio_position: 0.0,
        }), central_active: true, ..default()
    });
    app.world_mut().resource_mut::<VegetationPresetEditorState>().active_tab = PresetWorkspaceTab::AssetPreview;
    {
        let mut preview = app.world_mut().resource_mut::<AssetPreviewState>();
        preview.selected_path = Some("gltf/tree.glb".into());
        preview.content = AssetPreviewContent::Model { root: Entity::PLACEHOLDER, handle: Handle::default(),
            phase: crate::ui::asset_preview::types::AssetPreviewPhase::Ready };
    }
    press(&mut app, DocumentAction::OpenSelected);
    assert_eq!(app.world().resource::<VegetationPresetEditorState>().active_tab, PresetWorkspaceTab::Document);
    assert_eq!(app.world().resource::<DocumentState>().expanded.as_ref().unwrap().return_tab, PresetWorkspaceTab::AssetPreview);
    assert_eq!(app.world().resource::<DocumentState>().expanded.as_ref().unwrap().path, "说明.md");
    press(&mut app, DocumentAction::RestoreSidebar);
    assert_eq!(app.world().resource::<VegetationPresetEditorState>().active_tab, PresetWorkspaceTab::AssetPreview);
    assert_eq!(app.world().resource::<crate::ui::asset_preview::AssetPreviewWorkspace>().expanded.as_ref().unwrap().path, "gltf/tree.glb");
    press(&mut app, DocumentAction::OpenSelected);
    app.world_mut().remove_resource::<crate::ui::asset_preview::AssetPreviewWorkspace>();
    press(&mut app, DocumentAction::RestoreSidebar);
    assert_eq!(app.world().resource::<VegetationPresetEditorState>().active_tab, PresetWorkspaceTab::Scene);
}

fn sync_selection(app: &mut App) {
    app.world_mut()
        .run_system_once(session::sync_selection)
        .unwrap();
}

#[test]
fn asset_document_center_preserves_full_source_identity_and_saves_with_hidden_inspector() {
    let fixture = Fixture::new();
    fixture.write("中文段落\n".repeat(1_000));
    let mut app = workspace_app(&fixture);
    let (key, revision, expected) = {
        let mut state = app.world_mut().resource_mut::<DocumentState>();
        let doc = state.current_mut().unwrap();
        let complete = format!("{}末尾的草稿\n最后一个字", doc.source_text());
        doc.edit(&complete);
        (doc.source_key(), doc.revision, doc.pages.concat())
    };
    press(&mut app, DocumentAction::ToggleExpand);
    {
        let state = app.world().resource::<DocumentState>();
        let doc = state.current().unwrap();
        assert!(state.in_center());
        assert!(state.io.is_none() && state.pending_load.is_none());
        assert_eq!(doc.source_key(), key);
        assert_eq!(doc.revision, revision);
        assert_eq!(doc.mode, ViewMode::Edit);
        assert_eq!(doc.pages.concat(), expected);
    }
    app.world_mut()
        .resource_mut::<SceneEditorLayoutState>()
        .inspector_visible = false;
    app.world_mut()
        .resource_mut::<SceneEditorSidebarState>()
        .right = EditorRightSidebarTab::Scene;
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .selected_asset = Some("别的图片.png".into());
    app.world_mut().resource_mut::<AssetPreviewState>().content = AssetPreviewContent::None;
    sync_selection(&mut app);
    assert!(app.world().resource::<DocumentState>().owns_shortcuts());
    let mut keyboard = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    keyboard.press(KeyCode::ControlLeft);
    keyboard.press(KeyCode::KeyS);
    app.world_mut()
        .run_system_once(super::super::interaction::actions)
        .unwrap();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .clear();
    settle(&mut app);
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("说明.md")).unwrap(),
        expected
    );
    assert!(
        !app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .dirty()
    );
}

#[test]
fn asset_document_center_parks_across_tabs_and_only_explicit_open_replaces_path() {
    let fixture = Fixture::new();
    fixture.write("原文");
    std::fs::write(fixture.root.join("代码.rs"), "fn main() {}\n").unwrap();
    let mut app = workspace_app(&fixture);
    app.world_mut()
        .resource_mut::<DocumentState>()
        .current_mut()
        .unwrap()
        .edit("未保存草稿");
    press(&mut app, DocumentAction::ToggleExpand);
    let key = app
        .world()
        .resource::<DocumentState>()
        .current()
        .unwrap()
        .source_key();
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .selected_asset = Some("代码.rs".into());
    app.world_mut()
        .resource_mut::<AssetPreviewState>()
        .selected_path = Some("代码.rs".into());
    sync_selection(&mut app);
    assert_eq!(
        app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .source_key(),
        key
    );
    app.world_mut()
        .resource_mut::<VegetationPresetEditorState>()
        .active_tab = PresetWorkspaceTab::Domain("test.author", 17);
    sync_selection(&mut app);
    assert!(!app.world().resource::<DocumentState>().owns_shortcuts());
    assert!(!app.world().resource::<DocumentState>().visible);
    assert_eq!(
        app.world().resource::<DocumentState>().active.as_deref(),
        Some("说明.md")
    );
    let tab = app.world().resource::<DocumentState>().active_tab_id().unwrap();
    press(&mut app, DocumentAction::ActivateTab(tab));
    assert_eq!(
        app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .source_key(),
        key
    );
    press(&mut app, DocumentAction::OpenSelected);
    assert_eq!(
        app.world().resource::<DocumentState>().active.as_deref(),
        Some("代码.rs")
    );
    assert_eq!(
        app.world()
            .resource::<DocumentState>()
            .documents
            .iter()
            .find(|doc| doc.path == "说明.md")
            .unwrap()
            .source_text(),
        "未保存草稿"
    );
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("说明.md")).unwrap(),
        "原文"
    );
    settle(&mut app);
    assert_eq!(
        app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .format,
        DocumentFormat::Code
    );
}

#[test]
fn asset_document_center_restore_returns_to_prior_tab_without_discard_or_reload() {
    let fixture = Fixture::new();
    fixture.write("原文");
    let mut app = workspace_app(&fixture);
    // 扩展页是否仍打开由所属作者域发布；夹具只声明该页存在，不安装任何游戏工具。
    app.insert_resource(feathrise_editor_core::WorkspaceDomainState {
        open_tabs: vec![PresetWorkspaceTab::Domain("test.author", 17)],
        ..default()
    });
    app.world_mut()
        .resource_mut::<VegetationPresetEditorState>()
        .active_tab = PresetWorkspaceTab::Domain("test.author", 17);
    app.world_mut()
        .resource_mut::<DocumentState>()
        .current_mut()
        .unwrap()
        .edit("还原后保留草稿");
    let key = app
        .world()
        .resource::<DocumentState>()
        .current()
        .unwrap()
        .source_key();
    press(&mut app, DocumentAction::ToggleExpand);
    app.world_mut()
        .resource_mut::<SceneEditorLayoutState>()
        .toggle(crate::types::EditorLayoutAction::Focus);
    app.world_mut()
        .resource_mut::<SceneEditorSidebarState>()
        .right = EditorRightSidebarTab::Scene;
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .selected_asset = Some("其他文件.png".into());
    press(&mut app, DocumentAction::RestoreSidebar);
    let state = app.world().resource::<DocumentState>();
    assert!(!state.expanded());
    assert!(state.current().unwrap().dirty());
    assert_eq!(state.current().unwrap().source_key(), key);
    assert_eq!(state.current().unwrap().source_text(), "还原后保留草稿");
    assert!(state.io.is_none() && state.pending_load.is_none());
    assert_eq!(
        app.world()
            .resource::<VegetationPresetEditorState>()
            .active_tab,
        PresetWorkspaceTab::Domain("test.author", 17)
    );
    assert!(
        app.world()
            .resource::<AssetPreviewFocus>()
            .browsing(PresetWorkspaceTab::Domain("test.author", 17))
    );
    assert_eq!(
        app.world()
            .resource::<SceneEditorState>()
            .selected_asset
            .as_deref(),
        Some("说明.md")
    );
    assert_eq!(
        app.world().resource::<SceneEditorSidebarState>().right,
        EditorRightSidebarTab::Assets
    );
    assert_eq!(app.world().resource::<SceneEditorSidebarState>().inspector, crate::ui::sidebars::EditorInspectorTarget::Asset);
    assert!(
        app.world()
            .resource::<SceneEditorLayoutState>()
            .inspector_visible
    );
    assert!(
        app.world()
            .resource::<SceneEditorLayoutState>()
            .focus_restore
            .is_none()
    );
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("说明.md")).unwrap(),
        "原文"
    );
}

#[test]
fn asset_document_center_hidden_page_cannot_save_and_ime_blocks_expand() {
    let fixture = Fixture::new();
    fixture.write("原文");
    let mut app = workspace_app(&fixture);
    app.world_mut()
        .resource_mut::<DocumentState>()
        .current_mut()
        .unwrap()
        .edit("草稿");
    app.world_mut()
        .resource_mut::<EditorTextInputGuard>()
        .block_shortcuts = true;
    press(&mut app, DocumentAction::ToggleExpand);
    assert!(!app.world().resource::<DocumentState>().expanded());
    app.world_mut()
        .resource_mut::<EditorTextInputGuard>()
        .block_shortcuts = false;
    press(&mut app, DocumentAction::ToggleExpand);
    app.world_mut()
        .resource_mut::<VegetationPresetEditorState>()
        .active_tab = PresetWorkspaceTab::Scene;
    sync_selection(&mut app);
    let mut keyboard = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    keyboard.press(KeyCode::ControlLeft);
    keyboard.press(KeyCode::KeyS);
    app.world_mut()
        .run_system_once(super::super::interaction::actions)
        .unwrap();
    assert!(app.world().resource::<DocumentState>().io.is_none());
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("说明.md")).unwrap(),
        "原文"
    );
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .clear();
    let tab = app.world().resource::<DocumentState>().active_tab_id().unwrap();
    press(&mut app, DocumentAction::ActivateTab(tab));
    app.world_mut().resource_mut::<SceneEditorState>().open = false;
    sync_selection(&mut app);
    assert!(!app.world().resource::<DocumentState>().owns_shortcuts());
    assert_eq!(
        app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .source_text(),
        "草稿"
    );
    app.world_mut().resource_mut::<SceneEditorState>().open = true;
    sync_selection(&mut app);
    assert!(app.world().resource::<DocumentState>().owns_shortcuts());
}

#[test]
fn asset_document_center_clean_pinned_document_is_not_evicted_for_sidebar_load() {
    let fixture = Fixture::new();
    fixture.write("原文");
    let mut app = workspace_app(&fixture);
    press(&mut app, DocumentAction::ToggleExpand);
    let mut state = app.world_mut().resource_mut::<DocumentState>();
    for index in 1..MAX_DOCUMENTS {
        let mut doc = fixture.document();
        doc.path = format!("其他文档{index}.md");
        state.documents.push(doc);
    }
    state.request_load("新文档.md".into());
    assert_eq!(state.documents.len(), MAX_DOCUMENTS - 1);
    assert!(state.documents.iter().any(|doc| doc.path == "说明.md"));
    assert_eq!(state.pending_load.as_ref().unwrap().path, "新文档.md");
}

#[test]
fn asset_document_center_restore_falls_back_when_previous_preset_tab_was_closed() {
    let fixture = Fixture::new();
    fixture.write("原文");
    let mut app = workspace_app(&fixture);
    app.insert_resource(feathrise_editor_core::WorkspaceDomainState {open_tabs:vec![PresetWorkspaceTab::Domain("test.author",0)],..default()});
    app.world_mut()
        .resource_mut::<VegetationPresetEditorState>()
        .active_tab = PresetWorkspaceTab::Domain("test.author", 0);
    press(&mut app, DocumentAction::ToggleExpand);
    app.world_mut().resource_mut::<feathrise_editor_core::WorkspaceDomainState>().open_tabs.clear();
    press(&mut app, DocumentAction::RestoreSidebar);
    assert_eq!(
        app.world()
            .resource::<VegetationPresetEditorState>()
            .active_tab,
        PresetWorkspaceTab::Scene
    );
    assert!(app.world().resource::<DocumentState>().visible);
    assert_eq!(
        app.world().resource::<DocumentState>().active.as_deref(),
        Some("说明.md")
    );
}

#[test]
fn asset_multiple_document_tabs_switch_close_and_save_independent_drafts() {
    let fixture = Fixture::new();
    fixture.write("甲的原文");
    std::fs::write(fixture.root.join("配置.json"), "{\"value\":1}").unwrap();
    let mut app = workspace_app(&fixture);
    app.world_mut().resource_mut::<DocumentState>().current_mut().unwrap().edit("甲的草稿");
    press(&mut app, DocumentAction::ToggleExpand);
    let first = app.world().resource::<DocumentState>().expanded.as_ref().unwrap().id;
    app.world_mut().resource_mut::<SceneEditorState>().selected_asset = Some("配置.json".into());
    press(&mut app, DocumentAction::OpenSelected);
    settle(&mut app);
    let second = app.world().resource::<DocumentState>().expanded.as_ref().unwrap().id;
    app.world_mut().resource_mut::<DocumentState>().current_mut().unwrap().edit("{\"value\":2}");
    press(&mut app, DocumentAction::ActivateTab(first));
    assert_eq!(app.world().resource::<DocumentState>().current().unwrap().source_text(), "甲的草稿");
    press(&mut app, DocumentAction::Save);
    settle(&mut app);
    assert_eq!(std::fs::read_to_string(fixture.root.join("说明.md")).unwrap(), "甲的草稿");
    assert_eq!(std::fs::read_to_string(fixture.root.join("配置.json")).unwrap(), "{\"value\":1}");
    press(&mut app, DocumentAction::CloseTab(second));
    let state = app.world().resource::<DocumentState>();
    assert_eq!(state.tabs().count(), 1);
    assert_eq!(state.expanded.as_ref().unwrap().id, first);
    assert!(state.documents.iter().find(|doc| doc.path == "配置.json").unwrap().dirty());
    press(&mut app, DocumentAction::OpenSelected);
    assert_eq!(app.world().resource::<DocumentState>().current().unwrap().source_text(), "{\"value\":2}");
    let reopened = app.world().resource::<DocumentState>().expanded.as_ref().unwrap().id;
    press(&mut app, DocumentAction::CloseTab(reopened));
    assert_eq!(app.world().resource::<DocumentState>().expanded.as_ref().unwrap().id, first);
}

#[test]
fn asset_multiple_unloaded_tabs_reserve_capacity_without_evicting_drafts() {
    let fixture = Fixture::new();
    fixture.write("原文");
    let mut app = workspace_app(&fixture);
    {
        let mut state = app.world_mut().resource_mut::<DocumentState>();
        state.current_mut().unwrap().edit("已有草稿");
        for index in 1..7 {
            let mut doc = fixture.document();
            doc.path = format!("旧文档{index}.md");
            doc.edit("不能淘汰的草稿");
            state.documents.push(doc);
        }
    }
    app.world_mut().resource_mut::<SceneEditorState>().selected_asset = Some("还在读取.json".into());
    press(&mut app, DocumentAction::OpenSelected);
    let id = app.world().resource::<DocumentState>().expanded.as_ref().unwrap().id;
    app.world_mut().resource_mut::<SceneEditorState>().selected_asset = Some("第九份.json".into());
    press(&mut app, DocumentAction::OpenSelected);
    let state = app.world().resource::<DocumentState>();
    assert_eq!(state.tabs().count(), 1);
    assert_eq!(state.expanded.as_ref().unwrap().id, id);
    assert_eq!(state.documents.len(), 7);
    assert!(app.world().resource::<SceneEditorState>().status.contains("8份文档"));
}
