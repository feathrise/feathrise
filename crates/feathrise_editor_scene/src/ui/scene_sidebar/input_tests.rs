//! 运行原目录和删除入口，验证显示身份、模态及鼠标归还帧不重放旧按下。

use feathrise_editor_ui::UiInteraction;
use super::*;
use crate::authoring::DraftSaveTask;
use bevy::time::Real;
use feathrise_scene_content::ActiveDynamicScene;
use feathrise_scene_core::state::ScenePhase;
use std::time::Duration;

fn input_app() -> App {
    let mut app = sidebar_app();
    app.insert_resource(State::new(ScenePhase::Active))
        .init_resource::<Time<Real>>()
        .insert_resource(ActiveDynamicScene { scene_id: Some(feathrise_scene_content::SceneId::parse("SCENE-1").unwrap()) })
        .init_resource::<DraftSaveTask>()
        .add_systems(
            Update,
            (
                crate::scene_browser::handle_scene_browser_input,
                crate::ui::scene_deletion::handle_scene_deletion_buttons,
            )
                .chain()
                .before(sync_sidebar),
        );
    let catalog: SceneCatalogAsset = serde_json::from_value(serde_json::json!({ "schema_version": 2, "scenes": [
        {"id":"SCENE-1", "display_name":"第一张中文场景", "folder":"scenes/SCENE-1", "published":false},
        {"id":"SCENE-2", "display_name":"第二张中文场景", "folder":"scenes/SCENE-2", "published":false}
    ]})).unwrap();
    let handle = app
        .world_mut()
        .resource_mut::<Assets<SceneCatalogAsset>>()
        .add(catalog);
    app.world_mut().resource_mut::<SceneContentRegistry>().catalog = handle;
    register_manifest(&mut app, manifest("SCENE-2", "第二张中文场景"));
    app.world_mut()
        .resource_mut::<SceneBrowserState>()
        .document
        .folders
        .push(SceneFolderRecord {
            id: "FOLDER-1".into(),
            display_name: "第一目录".into(),
            parent_id: None,
        });
    Arc::make_mut(
        &mut app
            .world_mut()
            .resource_mut::<SceneBrowserIndexState>()
            .entries,
    )
    .push(entry(SceneBrowserItem::Folder("FOLDER-1".into()), "第一目录"));
    app.update();
    app
}

fn press(app: &mut App, action: EditorAction) {
    let entity = button(app, action);
    app.world_mut().entity_mut(entity).insert((UiInteraction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
    app.update();
}

fn release(app: &mut App, action: EditorAction) {
    let entity = button(app, action);
    app.world_mut().entity_mut(entity).insert((UiInteraction::None, feathrise_editor_ui::EditorButtonClick::default()));
    app.update();
}

fn scene_row(app: &mut App, id: &str, slot: usize) -> Entity {
    app.world_mut()
        .spawn((
            SceneEditorSceneRow {
                slot,
                item: Some(scene_item(id)),
            },
            UiInteraction::None,
            feathrise_editor_ui::EditorButtonClick::default(),
        ))
        .id()
}

fn click_row(app: &mut App, row: Entity) {
    app.world_mut().entity_mut(row).insert((UiInteraction::Pressed, feathrise_editor_ui::EditorButtonClick::default()));
    app.update();
    // 领域夹具不运行官方命中；显式声明本次释放已经通过原按钮身份校验。
    app.world_mut().entity_mut(row).insert((UiInteraction::None, feathrise_editor_ui::EditorButtonClick::triggered()));
    app.update();
}

#[test]
fn scene_row_selection_reveals_properties_without_entering_or_clearing_author_selection() {
    use crate::ui::sidebars::{EditorLeftSidebarTab, EditorRightSidebarTab};
    let mut app = input_app();
    let current = feathrise_scene_content::SceneId::parse("SCENE-1").unwrap();
    {
        let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
        editor.selected_scene_id = Some(current.clone());
        editor.tool = EditorTool::Water;
        editor.dirty = true;
    }
    {
        let mut sidebar = app.world_mut().resource_mut::<SceneEditorSidebarState>();
        sidebar.left = EditorLeftSidebarTab::Water;
        sidebar.right = EditorRightSidebarTab::Tools;
    }
    app.init_resource::<crate::viewport::LayerCandidateState>();
    let target = crate::viewport::LayerTarget::TerrainObject("stable-mesh".into());
    app.world_mut().resource_mut::<crate::viewport::LayerCandidateState>().selected = Some(target.clone());
    let revision = app.world().resource::<EditorAuthoringSession>().revision;
    let row = scene_row(&mut app, "SCENE-2", 1);
    click_row(&mut app, row);
    let editor = app.world().resource::<SceneEditorState>();
    assert_eq!(editor.selected_scene_id.as_ref(), Some(&current));
    assert!(editor.pending_scene_transition.is_none());
    assert!(!editor.new_scene_dialog_open);
    assert!(editor.dirty);
    assert_eq!(editor.tool, EditorTool::Water);
    assert_eq!(app.world().resource::<EditorAuthoringSession>().revision, revision);
    assert_eq!(app.world().resource::<crate::viewport::LayerCandidateState>().selected.as_ref(), Some(&target));
    assert_eq!(app.world().resource::<SceneEditorSidebarState>().right, EditorRightSidebarTab::SceneEntry);
    assert_eq!(app.world().resource::<SceneEditorSidebarState>().left, EditorLeftSidebarTab::Water);
}

#[test]
fn scene_properties_edit_uses_original_transaction_with_hidden_directory_and_rejects_stale_click() {
    use crate::ui::sidebars::EditorRightSidebarTab;
    let mut app = input_app();
    app.world_mut().resource_mut::<SceneBrowserState>().select_only(Some(scene_item("SCENE-2")));
    {
        let mut sidebar = app.world_mut().resource_mut::<SceneEditorSidebarState>();
        sidebar.navigator = EditorNavigatorTab::Assets;
        sidebar.reveal_scene_entry_properties();
    }
    let scene_id = feathrise_scene_content::SceneId::parse("SCENE-2").unwrap();
    let button = app.world_mut().spawn((SceneEditorActionButton(EditorAction::SceneFolderRename),
        ScenePropertiesEditBinding { scene_id: Some(scene_id.clone()), enabled: true }, UiInteraction::None)).id();
    app.world_mut().entity_mut(button).insert((UiInteraction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
    app.update();
    let editor = app.world().resource::<SceneEditorState>();
    assert!(editor.new_scene_dialog_open);
    assert_eq!(editor.new_scene_edit_target.as_ref().unwrap().id, scene_id);
    assert_eq!(editor.new_scene_name, "第二张中文场景");
    assert_eq!(editor.new_scene_notes, "真实备注");
    assert_eq!(app.world().resource::<SceneEditorSidebarState>().right, EditorRightSidebarTab::SceneEntry);
    {
        let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
        editor.new_scene_dialog_open = false;
        editor.new_scene_edit_target = None;
    }
    *app.world_mut().get_mut::<UiInteraction>(button).unwrap() = UiInteraction::None;
    app.update();
    app.world_mut().resource_mut::<SceneBrowserState>().select_only(Some(scene_item("SCENE-1")));
    app.world_mut().entity_mut(button).insert((UiInteraction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
    app.update();
    assert!(!app.world().resource::<SceneEditorState>().new_scene_dialog_open,
        "按下前已改选的旧编辑按钮不能改派到另一个场景");
}

/// 混合多选夹具使用真实按键资源与原输入链，不写入项目归类文件。
fn multi_input_app() -> App {
    bevy::tasks::AsyncComputeTaskPool::get_or_init(Default::default);
    let mut app = input_app();
    app.init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .add_systems(
            Update,
            crate::scene_browser::update_scene_browser_drag
                .before(crate::scene_browser::handle_scene_browser_input),
        );
    let mut window = Window::default();
    window.set_physical_cursor_position(Some(Vec2::new(10.0, 10.0).into()));
    app.world_mut().spawn((window, bevy::window::PrimaryWindow));
    app
}

fn folder_row(app: &mut App) -> Entity {
    app.world_mut()
        .spawn((
            SceneEditorSceneRow {
                slot: 2,
                item: Some(SceneBrowserItem::Folder("FOLDER-1".into())),
            },
            UiInteraction::None,
            feathrise_editor_ui::EditorButtonClick::default(),
        ))
        .id()
}

fn settle_selection(app: &mut App) {
    for _ in 0..1000 {
        app.update();
        if !app.world().resource::<SceneBrowserState>().selection_pending {
            return;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    panic!("后台连选应完成");
}

#[test]
fn control_selects_scenes_and_folders_together_without_opening_or_deleting() {
    let mut app = multi_input_app();
    let first = scene_row(&mut app, "SCENE-1", 0);
    let second = scene_row(&mut app, "SCENE-2", 1);
    let folder = folder_row(&mut app);
    click_row(&mut app, first);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ControlLeft);
    click_row(&mut app, second);
    click_row(&mut app, folder);
    assert_eq!(
        app.world().resource::<SceneBrowserState>().selection_count(),
        3
    );
    assert_eq!(
        text(&mut app, SidebarField::SelectedName),
        "2 个场景、1 个文件夹"
    );
    assert!(
        !app.world()
            .resource::<SceneBrowserState>()
            .expanded_folder_ids
            .contains("FOLDER-1")
    );
    assert!(
        app.world()
            .resource::<SceneEditorState>()
            .pending_scene_transition
            .is_none()
    );
    assert!(
        app.world()
            .resource::<SceneEditorState>()
            .scene_list_selection_id
            .is_none()
    );
    for action in [
        EditorAction::EnterSelectedScene,
        EditorAction::SceneFolderRename,
        EditorAction::DeleteScene,
    ] {
        press(&mut app, action);
        release(&mut app, action);
    }
    assert!(
        app.world()
            .resource::<SceneDeletionTask>()
            .confirmation
            .is_none()
    );
    assert!(app.world().resource::<SceneBrowserState>().dialog.is_none());
    click_row(&mut app, first);
    click_row(&mut app, second);
    click_row(&mut app, folder);
    assert_eq!(
        app.world().resource::<SceneBrowserState>().selection_count(),
        0
    );
    assert!(!app.world().resource::<SceneFolderPersistence>().busy());
}

#[test]
fn shift_uses_the_complete_visible_projection_beyond_the_row_pool_and_ctrl_shift_adds() {
    let mut app = multi_input_app();
    let visible: Vec<_> = (1..=120)
        .map(|id| entry(scene_item(&format!("SCENE-{id}")), "中文场景"))
        .collect();
    app.insert_resource(published_index(visible));
    let first = scene_row(&mut app, "SCENE-1", 0);
    let last = scene_row(&mut app, "SCENE-120", 27);
    click_row(&mut app, first);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ShiftRight);
    app.world_mut()
        .resource_mut::<SceneBrowserIndexState>()
        .rebuilding = true;
    click_row(&mut app, last);
    assert_eq!(
        app.world().resource::<SceneBrowserState>().selection_count(),
        1
    );
    assert!(!app.world().resource::<SceneBrowserState>().selection_pending);
    app.world_mut()
        .resource_mut::<SceneBrowserIndexState>()
        .rebuilding = false;
    click_row(&mut app, last);
    settle_selection(&mut app);
    assert_eq!(
        app.world().resource::<SceneBrowserState>().selection_count(),
        120
    );
    assert!(
        app.world()
            .resource::<SceneBrowserState>()
            .item_selected(&scene_item("SCENE-60"))
    );
    // 改到靠前的可见行时，仍使用原锚点；Ctrl+Shift把范围加入原选集。
    let middle = scene_row(&mut app, "SCENE-30", 2);
    click_row(&mut app, middle);
    settle_selection(&mut app);
    assert_eq!(
        app.world().resource::<SceneBrowserState>().selection_count(),
        30
    );
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ControlRight);
    click_row(&mut app, last);
    settle_selection(&mut app);
    assert_eq!(
        app.world().resource::<SceneBrowserState>().selection_count(),
        120
    );
    assert!(
        app.world()
            .resource::<SceneEditorState>()
            .pending_scene_transition
            .is_none()
    );
    assert!(!app.world().resource::<SceneEditorState>().dirty);
}

#[test]
fn pressing_a_selected_member_keeps_the_group_for_drag_and_plain_release_collapses_it() {
    let mut app = multi_input_app();
    let first = scene_row(&mut app, "SCENE-1", 0);
    let second = scene_row(&mut app, "SCENE-2", 1);
    click_row(&mut app, first);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ControlLeft);
    click_row(&mut app, second);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(KeyCode::ControlLeft);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.world_mut().entity_mut(first).insert((UiInteraction::Pressed, feathrise_editor_ui::EditorButtonClick::default()));
    app.update();
    assert_eq!(
        app.world().resource::<SceneBrowserState>().selection_count(),
        2
    );
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    app.world_mut().entity_mut(first).insert((UiInteraction::Hovered, feathrise_editor_ui::EditorButtonClick::triggered()));
    app.update();
    assert_eq!(
        app.world().resource::<SceneBrowserState>().selection_count(),
        1
    );
    assert_eq!(
        app.world().resource::<SceneBrowserState>().selected_item,
        Some(scene_item("SCENE-1"))
    );
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ControlLeft);
    click_row(&mut app, second);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(KeyCode::ControlLeft);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.world_mut().entity_mut(first).insert((UiInteraction::Pressed, feathrise_editor_ui::EditorButtonClick::default()));
    app.update();
    let window = app
        .world_mut()
        .query_filtered::<Entity, With<bevy::window::PrimaryWindow>>()
        .single(app.world())
        .unwrap();
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .set_physical_cursor_position(Some(Vec2::new(25.0, 10.0).into()));
    app.update();
    assert!(app.world().resource::<SceneBrowserDragState>().active);
    assert_eq!(
        app.world()
            .resource::<SceneBrowserDragState>()
            .selection
            .as_ref()
            .unwrap()
            .items
            .len(),
        2
    );
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    *app.world_mut().get_mut::<UiInteraction>(first).unwrap() = UiInteraction::None;
    app.update(); // 空白释放只取消拖放，仍保留整组选集。
    assert_eq!(
        app.world().resource::<SceneBrowserState>().selection_count(),
        2
    );
    assert!(!app.world().resource::<SceneFolderPersistence>().busy());
}

#[test]
fn late_shift_result_is_discarded_after_projection_changes_or_hiding() {
    for hidden in [false, true] {
        let mut app = multi_input_app();
        let first = scene_row(&mut app, "SCENE-1", 0);
        let second = scene_row(&mut app, "SCENE-2", 1);
        click_row(&mut app, first);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::ShiftLeft);
        click_row(&mut app, second);
        assert!(app.world().resource::<SceneBrowserState>().selection_pending);
        if hidden {
            app.world_mut().resource_mut::<SceneEditorSidebarState>().navigator =
                EditorNavigatorTab::SceneTree;
        } else {
            app.world_mut()
                .resource_mut::<SceneBrowserState>()
                .document_revision += 1;
        }
        settle_selection(&mut app);
        assert_eq!(
            app.world().resource::<SceneBrowserState>().selected_item,
            Some(scene_item("SCENE-1"))
        );
        assert_eq!(
            app.world().resource::<SceneBrowserState>().selection_count(),
            1
        );
        assert!(
            app.world()
                .resource::<SceneBrowserDragState>()
                .press
                .is_none()
        );
    }
}

#[test]
fn double_clicking_a_group_member_collapses_then_opens_the_clicked_scene() {
    let mut app = multi_input_app();
    let row = scene_row(&mut app, "SCENE-1", 0);
    {
        let mut browser = app.world_mut().resource_mut::<SceneBrowserState>();
        browser.select_only(Some(scene_item("SCENE-1")));
        browser.toggle_selection(scene_item("SCENE-2"));
    }
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.world_mut().entity_mut(row).insert((UiInteraction::Pressed, feathrise_editor_ui::EditorButtonClick::default()));
    app.update();
    assert_eq!(
        app.world().resource::<SceneBrowserState>().selection_count(),
        2
    );
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    app.world_mut().entity_mut(row).insert((UiInteraction::Hovered, feathrise_editor_ui::EditorButtonClick::triggered()));
    app.update();
    assert_eq!(
        app.world().resource::<SceneBrowserState>().selection_count(),
        1
    );
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.world_mut().entity_mut(row).insert((UiInteraction::Pressed, feathrise_editor_ui::EditorButtonClick::default()));
    app.update();
    assert!(app.world().resource::<SceneEditorState>().pending_scene_transition.is_none(),
        "第二次按住仍不能提前进入场景");
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
    app.world_mut().entity_mut(row).insert((UiInteraction::Hovered, feathrise_editor_ui::EditorButtonClick::triggered()));
    app.update();
    assert_eq!(
        app.world()
            .resource::<SceneEditorState>()
            .pending_scene_transition,
        Some(SceneId::parse("SCENE-1").unwrap())
    );
    assert!(
        app.world()
            .resource::<SceneBrowserDragState>()
            .press
            .is_none()
    );
}

#[test]
fn folder_click_waits_for_release_and_drag_keeps_the_source_branch_expanded() {
    use bevy::window::PrimaryWindow;
    let mut app = input_app();
    app.init_resource::<ButtonInput<MouseButton>>().add_systems(
        Update,
        crate::scene_browser::update_scene_browser_drag
            .before(crate::scene_browser::handle_scene_browser_input),
    );
    let mut window = Window::default();
    window.set_physical_cursor_position(Some(Vec2::new(10.0, 10.0).into()));
    let window = app.world_mut().spawn((window, PrimaryWindow)).id();
    let row = app
        .world_mut()
        .spawn((
            SceneEditorSceneRow {
                slot: 0,
                item: Some(SceneBrowserItem::Folder("FOLDER-1".into())),
            },
            UiInteraction::None,
            feathrise_editor_ui::EditorButtonClick::default(),
        ))
        .id();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.world_mut().entity_mut(row).insert((UiInteraction::Pressed, feathrise_editor_ui::EditorButtonClick::default()));
    app.update();
    assert!(
        !app.world()
            .resource::<SceneBrowserState>()
            .expanded_folder_ids
            .contains("FOLDER-1")
    );
    assert!(
        app.world()
            .resource::<SceneBrowserDragState>()
            .press
            .is_some()
    );
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    app.world_mut().entity_mut(row).insert((UiInteraction::Hovered, feathrise_editor_ui::EditorButtonClick::triggered()));
    app.update();
    assert!(
        app.world()
            .resource::<SceneBrowserState>()
            .expanded_folder_ids
            .contains("FOLDER-1")
    );
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.world_mut().entity_mut(row).insert((UiInteraction::Pressed, feathrise_editor_ui::EditorButtonClick::default()));
    app.update();
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .set_physical_cursor_position(Some(Vec2::new(20.0, 10.0).into()));
    app.update();
    assert!(app.world().resource::<SceneBrowserDragState>().active);
    assert!(
        app.world()
            .resource::<SceneBrowserState>()
            .expanded_folder_ids
            .contains("FOLDER-1")
    );
    assert!(!app.world().resource::<SceneFolderPersistence>().busy());
}

#[test]
fn same_scene_double_click_queues_original_transition_without_loading_on_single_click() {
    let mut app = input_app();
    let row = scene_row(&mut app, "SCENE-2", 0);
    click_row(&mut app, row);
    assert_eq!(
        app.world().resource::<SceneBrowserState>().selected_item,
        Some(scene_item("SCENE-2"))
    );
    assert!(
        app.world()
            .resource::<SceneEditorState>()
            .pending_scene_transition
            .is_none()
    );
    assert!(!app.world().resource::<SceneEditorState>().dirty);
    app.world_mut()
        .resource_mut::<Time<Real>>()
        .advance_by(Duration::from_millis(100));
    click_row(&mut app, row);
    let editor = app.world().resource::<SceneEditorState>();
    assert_eq!(
        editor.pending_scene_transition,
        Some(SceneId::parse("SCENE-2").unwrap())
    );
    assert!(editor.selected_scene_id.is_none()); // 双击只登记原异步切换场景请求，不假装已加载成功。
    assert!(
        app.world()
            .resource::<SceneBrowserDragState>()
            .press
            .is_none()
    );
    assert!(text(&mut app, SidebarField::OperationFeedback).contains("完整准备"));
}

#[test]
fn slow_clicks_other_targets_and_directory_generations_do_not_open_scenes() {
    for change in [
        "slow",
        "other_scene",
        "root",
        "folder",
        "catalog",
        "projection",
    ] {
        let mut app = input_app();
        let row = scene_row(&mut app, "SCENE-1", 0);
        click_row(&mut app, row);
        app.world_mut()
            .resource_mut::<Time<Real>>()
            .advance_by(Duration::from_millis(100));
        match change {
            "slow" => app
                .world_mut()
                .resource_mut::<Time<Real>>()
                .advance_by(Duration::from_millis(300)),
            "other_scene" => {
                app.world_mut()
                    .get_mut::<SceneEditorSceneRow>(row)
                    .unwrap()
                    .item = Some(scene_item("SCENE-2"));
            }
            "root" => {
                let root = app
                    .world_mut()
                    .spawn((SceneEditorSceneRootRow, UiInteraction::None))
                    .id();
                click_row(&mut app, root);
            }
            "folder" => {
                let folder = app
                    .world_mut()
                    .spawn((
                        SceneEditorSceneRow {
                            slot: 1,
                            item: Some(SceneBrowserItem::Folder("FOLDER-1".into())),
                        },
                        UiInteraction::None,
            feathrise_editor_ui::EditorButtonClick::default(),
                    ))
                    .id();
                click_row(&mut app, folder);
            }
            "catalog" => {
                app.world_mut()
                    .resource_mut::<SceneContentRegistry>()
                    .revision += 1;
                let mut index = app.world_mut().resource_mut::<SceneBrowserIndexState>();
                index
                    .applied_projection
                    .as_mut()
                    .unwrap()
                    .0
                    .catalog_revision += 1;
                index.desired_projection = index.applied_projection.clone();
            }
            "projection" => {
                app.world_mut()
                    .resource_mut::<SceneBrowserState>()
                    .expansion_revision += 1;
                let mut index = app.world_mut().resource_mut::<SceneBrowserIndexState>();
                index.applied_projection.as_mut().unwrap().1 += 1;
                index.desired_projection = index.applied_projection.clone();
            }
            _ => unreachable!(),
        }
        click_row(&mut app, row);
        assert!(
            app.world()
                .resource::<SceneEditorState>()
                .pending_scene_transition
                .is_none(),
            "{change}"
        );
    }
}

#[test]
fn blocked_double_click_is_consumed_and_next_press_starts_a_new_sequence() {
    for blocker in ["numeric", "hidden", "modal", "drag", "closed"] {
        let mut app = input_app();
        let row = scene_row(&mut app, "SCENE-1", 0);
        click_row(&mut app, row);
        match blocker {
            "numeric" => app
                .world_mut()
                .resource_mut::<SceneEditorNumericScrubMouseGuard>()
                .advance(true, true, false),
            "hidden" => {
                app.world_mut().resource_mut::<SceneEditorSidebarState>().navigator =
                    EditorNavigatorTab::SceneTree
            }
            "modal" => {
                app.world_mut()
                    .resource_mut::<SceneEditorState>()
                    .new_scene_dialog_open = true
            }
            "drag" => {
                app.world_mut()
                    .resource_mut::<SceneBrowserDragState>()
                    .blocked = true
            }
            "closed" => app.world_mut().resource_mut::<SceneEditorState>().open = false,
            _ => unreachable!(),
        }
        click_row(&mut app, row);
        app.world_mut()
            .resource_mut::<SceneEditorNumericScrubMouseGuard>()
            .advance(false, false, false);
        app.world_mut().resource_mut::<SceneEditorSidebarState>().navigator = EditorNavigatorTab::Scenes;
        app.world_mut()
            .resource_mut::<SceneEditorState>()
            .new_scene_dialog_open = false;
        app.world_mut().resource_mut::<SceneEditorState>().open = true;
        app.world_mut()
            .resource_mut::<SceneBrowserDragState>()
            .blocked = false;
        app.update();
        assert!(
            app.world()
                .resource::<SceneEditorState>()
                .pending_scene_transition
                .is_none(),
            "{blocker}"
        );
        click_row(&mut app, row);
        assert!(
            app.world()
                .resource::<SceneEditorState>()
                .pending_scene_transition
                .is_none(),
            "{blocker}"
        );
        app.world_mut()
            .resource_mut::<Time<Real>>()
            .advance_by(Duration::from_millis(100));
        click_row(&mut app, row);
        assert_eq!(
            app.world()
                .resource::<SceneEditorState>()
                .pending_scene_transition,
            Some(SceneId::parse("SCENE-1").unwrap()),
            "{blocker}"
        );
    }
}

#[test]
fn double_click_preserves_dirty_confirmation_and_does_not_replay_after_saving() {
    let mut app = input_app();
    let row = scene_row(&mut app, "SCENE-2", 0);
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .selected_scene_id = Some(SceneId::parse("SCENE-1").unwrap());
    app.world_mut().resource_mut::<SceneEditorState>().dirty = true;
    click_row(&mut app, row);
    click_row(&mut app, row);
    let editor = app.world().resource::<SceneEditorState>();
    assert!(editor.pending_scene_transition.is_none());
    assert_eq!(
        editor
            .scene_switch_confirmation
            .as_ref()
            .unwrap()
            .target_scene_id,
        SceneId::parse("SCENE-2").unwrap()
    );

    let mut app = input_app();
    let row = scene_row(&mut app, "SCENE-2", 0);
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .saving = true;
    click_row(&mut app, row);
    click_row(&mut app, row);
    assert!(
        app.world()
            .resource::<SceneEditorState>()
            .pending_scene_transition
            .is_none()
    );
    assert!(text(&mut app, SidebarField::OperationFeedback).contains("正在保存"));
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .saving = false;
    app.update();
    click_row(&mut app, row);
    assert!(
        app.world()
            .resource::<SceneEditorState>()
            .pending_scene_transition
            .is_none()
    );
}

#[test]
fn actual_drag_threshold_cancels_click_sequence_without_opening_or_writing() {
    use bevy::window::PrimaryWindow;
    let mut app = input_app();
    app.init_resource::<ButtonInput<MouseButton>>().add_systems(
        Update,
        crate::scene_browser::update_scene_browser_drag
            .before(crate::scene_browser::handle_scene_browser_input),
    );
    let mut window = Window::default();
    window.set_physical_cursor_position(Some(Vec2::new(10.0, 10.0).into()));
    let window = app.world_mut().spawn((window, PrimaryWindow)).id();
    let row = scene_row(&mut app, "SCENE-1", 0);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.world_mut().entity_mut(row).insert((UiInteraction::Pressed, feathrise_editor_ui::EditorButtonClick::default()));
    app.update();
    assert!(
        app.world()
            .resource::<SceneBrowserDragState>()
            .press
            .is_some()
    );
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .set_physical_cursor_position(Some(Vec2::new(20.0, 10.0).into()));
    app.update();
    assert!(app.world().resource::<SceneBrowserDragState>().active);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    *app.world_mut().get_mut::<UiInteraction>(row).unwrap() = UiInteraction::None;
    app.update(); // 无有效文件夹落点，按原拖拽流程取消；release帧仍隔离鼠标。
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    app.update();
    assert!(
        !app.world()
            .resource::<SceneBrowserDragState>()
            .blocks_other_mouse()
    );
    app.world_mut()
        .resource_mut::<Time<Real>>()
        .advance_by(Duration::from_millis(100));
    click_row(&mut app, row);
    assert!(
        app.world()
            .resource::<SceneEditorState>()
            .pending_scene_transition
            .is_none()
    );
    assert!(!app.world().resource::<SceneFolderPersistence>().busy());
    assert!(
        app.world()
            .resource::<SceneBrowserState>()
            .document
            .scene_folders
            .is_empty()
    );
}

#[test]
fn stale_selection_press_is_consumed_before_original_enter_and_not_replayed() {
    let mut app = input_app();
    select(&mut app, Some(scene_item("SCENE-1")));
    app.world_mut()
        .resource_mut::<SceneBrowserState>()
        .selected_item = Some(scene_item("SCENE-2"));
    press(&mut app, EditorAction::EnterSelectedScene);
    assert!(
        app.world()
            .resource::<SceneEditorState>()
            .pending_scene_transition
            .is_none()
    );
    app.update();
    assert!(
        app.world()
            .resource::<SceneEditorState>()
            .pending_scene_transition
            .is_none()
    );
    release(&mut app, EditorAction::EnterSelectedScene);
    press(&mut app, EditorAction::EnterSelectedScene);
    assert_eq!(
        app.world()
            .resource::<SceneEditorState>()
            .pending_scene_transition,
        Some(SceneId::parse("SCENE-2").unwrap())
    );
    assert_eq!(
        app.world().resource::<SceneEditorState>().selected_scene_id,
        None
    );
    assert!(text(&mut app, SidebarField::OperationFeedback).contains("完整准备"));
}

#[test]
fn numeric_guard_and_hidden_page_press_are_not_replayed() {
    let mut app = input_app();
    select(&mut app, Some(scene_item("SCENE-2")));
    app.world_mut()
        .resource_mut::<SceneEditorNumericScrubMouseGuard>()
        .advance(true, true, false);
    press(&mut app, EditorAction::SceneFolderRename);
    app.world_mut()
        .resource_mut::<SceneEditorNumericScrubMouseGuard>()
        .advance(false, false, false);
    app.update();
    assert!(!app.world().resource::<SceneEditorState>().new_scene_dialog_open);
    release(&mut app, EditorAction::SceneFolderRename);
    app.world_mut().resource_mut::<SceneEditorSidebarState>().navigator = EditorNavigatorTab::SceneTree;
    press(&mut app, EditorAction::SceneFolderRename);
    app.world_mut().resource_mut::<SceneEditorSidebarState>().navigator = EditorNavigatorTab::Scenes;
    app.update();
    assert!(!app.world().resource::<SceneEditorState>().new_scene_dialog_open);
    release(&mut app, EditorAction::SceneFolderRename);
    press(&mut app, EditorAction::SceneFolderRename);
    let editor = app.world().resource::<SceneEditorState>();
    assert!(editor.new_scene_dialog_open);
    assert_eq!(
        editor.new_scene_edit_target.as_ref().unwrap().id,
        SceneId::parse("SCENE-2").unwrap()
    );
    assert_eq!(editor.new_scene_notes, "真实备注");
    assert!(!editor.dirty);
    assert!(text(&mut app, SidebarField::OperationFeedback).contains("编辑场景资料"));
}

#[test]
fn folder_rename_uses_original_frozen_dialog_without_writing() {
    let mut app = input_app();
    let item = SceneBrowserItem::Folder("FOLDER-1".into());
    let mut entries = app
        .world()
        .resource::<SceneBrowserIndexState>()
        .entries
        .clone();
    app.insert_resource(published_index(
        std::mem::take(&mut entries).as_ref().clone(),
    ));
    select(&mut app, Some(item));
    press(&mut app, EditorAction::SceneFolderRename);
    let browser = app.world().resource::<SceneBrowserState>();
    let dialog = browser.dialog.as_ref().unwrap();
    assert_eq!(dialog.mode, SceneFolderDialogMode::Rename);
    assert_eq!(dialog.target_folder_id.as_deref(), Some("FOLDER-1"));
    assert_eq!(dialog.name, "第一目录");
    assert!(!app.world().resource::<SceneFolderPersistence>().busy());
    assert_eq!(browser.document.folders[0].display_name, "第一目录");
}

#[test]
fn scene_delete_still_uses_original_confirmation_and_dirty_protection() {
    let mut app = input_app();
    select(&mut app, Some(scene_item("SCENE-2")));
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .scene_list_selection_id = Some(SceneId::parse("SCENE-1").unwrap()); // 兼容高亮字段即使落后，也只能确认本页明确显示的SCENE-2。
    app.world_mut().resource_mut::<SceneEditorState>().dirty = true;
    press(&mut app, EditorAction::DeleteScene);
    assert!(
        app.world()
            .resource::<SceneDeletionTask>()
            .confirmation
            .is_none()
    );
    assert!(text(&mut app, SidebarField::OperationFeedback).contains("未保存修改"));
    release(&mut app, EditorAction::DeleteScene);
    app.world_mut().resource_mut::<SceneEditorState>().dirty = false;
    app.update();
    press(&mut app, EditorAction::DeleteScene);
    let deletion = app.world().resource::<SceneDeletionTask>();
    assert_eq!(
        deletion.confirmation.as_ref().unwrap().scene_id,
        SceneId::parse("SCENE-2").unwrap()
    );
    assert!(deletion.pending_start.is_none() && deletion.task.is_none());
    assert!(text(&mut app, SidebarField::OperationFeedback).contains("尚未删除"));
}

#[test]
fn repeated_original_error_is_captured_again_after_context_change() {
    let mut app = input_app();
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .saving = true;
    select(&mut app, Some(scene_item("SCENE-1")));
    press(&mut app, EditorAction::EnterSelectedScene);
    let expected = text(&mut app, SidebarField::OperationFeedback);
    assert!(expected.contains("正在保存"));
    release(&mut app, EditorAction::EnterSelectedScene);
    select(&mut app, Some(scene_item("SCENE-2")));
    assert!(text(&mut app, SidebarField::OperationFeedback).is_empty());
    press(&mut app, EditorAction::EnterSelectedScene);
    assert_eq!(text(&mut app, SidebarField::OperationFeedback), expected);
}
