//! 空世界编辑器的直接启动、真实进图门、键盘/按钮关闭及无场景 RTT 生命周期回归。

use feathrise_editor_ui::UiInteraction;
use bevy::ecs::system::RunSystemOnce;
use bevy::ui::widget::ViewportNode;
use feathrise_scene_core::input::{InputGateState, EditorInputState};
use feathrise_scene_content::{ActiveDynamicScene, SceneCatalogAsset, SceneId};
use feathrise_scene_core::state::{EditorStartup, ScenePhase, LoadingProgress, SceneStartupGate};

use super::*;
use crate::authoring::{
    BoundaryEditState, DraftLoadTask, EditorAuthoringSession, TerrainBrushStroke,
    TerrainLiveApplyState, TerrainPreviewState, WaterBrushStroke, WaterFileHandoff,
    WaterNetworkEditState, WaterPreviewState,
};
use crate::components::{SceneEditorCamera, SceneEditorViewport};
use crate::resources::{
    EditorAssetCatalog, SceneBrowserState, SceneCreationTask, SceneDeletionTask,
    SceneEditorNumericScrubMouseGuard, SceneEditorState, SceneEditorViewportState, SceneFolderPersistence,
};
use crate::ui::systems::{sync_scene_editor_blocker, toggle_scene_editor};

fn ready(world: &mut World) -> bool {
    world
        .run_system_once(|session: SceneEditorWorldSession| session.ready())
        .unwrap()
}

#[test]
fn editor_world_session_requires_a_real_ready_scene_not_a_catalog_selection() {
    let mut world = World::new();
    world.insert_resource(EditorStartup);
    world.insert_resource(SceneEditorState {
        selected_scene_id: Some(SceneId::parse("HN-01").unwrap()),
        ..default()
    });
    assert!(!ready(&mut world));
    for scene in [
        ScenePhase::Idle,
        ScenePhase::Loading,
        ScenePhase::Active,
    ] {
        world.insert_resource(State::new(scene));
        assert!(!ready(&mut world));
    }
    world.insert_resource(ActiveDynamicScene {
        scene_id: Some(SceneId::parse("MAP-editor-test").unwrap()),
    });
    assert!(ready(&mut world));
    world.insert_resource(SceneStartupGate::waiting_for_collision(
        ScenePhase::Active,
    ));
    assert!(!ready(&mut world));
    world.resource_mut::<SceneStartupGate>().ready = true;
    assert!(ready(&mut world));
    world.insert_resource(LoadingProgress::default());
    assert!(!ready(&mut world));
    world.remove_resource::<LoadingProgress>();
    assert!(ready(&mut world));
    world.remove_resource::<ActiveDynamicScene>();
    assert!(!ready(&mut world)); // 缺失SceneId时不能把目录高亮项当成当前世界。
}

fn keyboard_app(editor_startup: bool) -> App {
    let mut app = App::new();
    app.init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<feathrise_editor_ui::ActiveNumericInput>()
        .init_resource::<SceneEditorNumericScrubMouseGuard>()
        .init_resource::<WaterNetworkEditState>()
        .init_resource::<InputGateState>()
        .init_resource::<EditorInputState>()
        .insert_resource(ActiveDynamicScene { scene_id: Some(SceneId::parse("MAP-editor-test").unwrap()) })
        .init_resource::<SceneEditorState>()
        .insert_resource(State::new(ScenePhase::Idle))
        .add_systems(Update, (toggle_scene_editor, sync_scene_editor_blocker).chain());
    if editor_startup {
        app.insert_resource(EditorStartup);
    }
    app
}

#[test]
fn editor_world_startup_allows_keyboard_close_and_reopen_without_auto_reopening() {
    let mut app = keyboard_app(true);
    init_close_resources(&mut app);
    app.init_resource::<VegetationPresetEditorState>()
        .add_systems(
            Update,
            crate::ui::close::finish_scene_editor_close
                .after(toggle_scene_editor)
                .before(sync_scene_editor_blocker),
        );
    app.update();
    let editor = app.world().resource::<SceneEditorState>();
    assert!(editor.open);
    assert!(editor.selected_scene_id.is_none());
    assert!(editor.scene_list_selection_id.is_none());
    assert!(app.world().resource::<EditorInputState>().scene_editor_open);
    app.world_mut().resource_mut::<VegetationPresetEditorState>().active_tab = PresetWorkspaceTab::Domain("fixture.domain", 0);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::F12);
    app.update();
    let editor = app.world().resource::<SceneEditorState>();
    assert!(!editor.open);
    assert!(!editor.close_requested);
    assert!(editor.empty_workspace_closed);
    assert!(app.world().contains_resource::<EditorStartup>()); // 空场景继续禁止创建开始菜单。
    assert!(!app.world().resource::<EditorInputState>().scene_editor_open);
    assert_eq!(app.should_exit(), None);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .reset_all();
    for _ in 0..12 {
        app.insert_resource(EditorStartup); // 重复写入无菜单模式也不能被当成新的自动打开请求。
        app.update();
        assert!(!app.world().resource::<SceneEditorState>().open);
        assert!(!app.world().resource::<SceneEditorState>().close_requested);
    }
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::F12);
    app.update();
    assert!(app.world().resource::<SceneEditorState>().open); // 空场景非game_mode也允许再次打开F12。
    assert!(!app.world().resource::<SceneEditorState>().empty_workspace_closed);
    assert!(!app.world().resource::<SceneEditorState>().close_requested);
    assert_eq!(app.world().resource::<VegetationPresetEditorState>().active_tab, PresetWorkspaceTab::Domain("fixture.domain", 0));
}

#[test]
fn editor_world_ordinary_startup_keeps_editor_closed_until_manual_f12() {
    let mut app = keyboard_app(false);
    app.update();
    assert!(!app.world().resource::<SceneEditorState>().open);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::F12);
    app.update();
    assert!(app.world().resource::<SceneEditorState>().open);
}

#[test]
fn editor_world_paused_application_allows_author_round_trip_without_clearing_pause() {
    let mut app = keyboard_app(false);
    init_close_resources(&mut app);
    app.insert_resource(State::new(ScenePhase::Active))
        .add_systems(
            Update,
            crate::ui::close::finish_scene_editor_close
                .after(toggle_scene_editor)
                .before(sync_scene_editor_blocker),
        );
    app.world_mut().resource_mut::<InputGateState>().time_paused = true;
    app.world_mut().resource_mut::<InputGateState>().camera_control_allowed = false;
    // 真实F12入口、原关闭事务和公共占用同步共同执行，应用暂停不能把作者锁在试玩里。
    for expected_open in [true, false, true] {
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().reset_all();
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::F12);
        app.update();
        let editor = app.world().resource::<SceneEditorState>();
        assert_eq!(editor.open, expected_open);
        assert!(!editor.close_requested);
        assert_eq!(editor.selected_scene_id, Some(SceneId::parse("MAP-editor-test").unwrap()));
        let blocks = app.world().resource::<EditorInputState>();
        assert_eq!(blocks.scene_editor_open, expected_open);
        assert!(app.world().resource::<InputGateState>().time_paused); // 作者往返不得修改应用暂停事实。
        assert!(!app.world().resource::<InputGateState>().camera_control_allowed);
    }
}

#[test]
fn editor_world_scene_focus_permission_controls_entry_without_changing_application_state() {
    for focus_available in [false, true] {
        let mut app = keyboard_app(false);
        app.insert_resource(State::new(ScenePhase::Active));
        app.world_mut().resource_mut::<InputGateState>().time_paused = true;
        app.world_mut().resource_mut::<InputGateState>().scene_focus_available = focus_available;
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::F12);
        app.update();
        assert_eq!(app.world().resource::<SceneEditorState>().open, focus_available);
        assert_eq!(app.world().resource::<InputGateState>().scene_focus_available, focus_available);
        assert!(app.world().resource::<InputGateState>().time_paused); // 只交接作者焦点，不改变应用暂停。
    }
}

#[test]
fn editor_world_focus_permission_does_not_bypass_modal_or_loading_entry_guards() {
    let mut app = keyboard_app(false);
    app.insert_resource(State::new(ScenePhase::Active));
    app.world_mut().resource_mut::<InputGateState>().time_paused = true;
    app.world_mut().resource_mut::<InputGateState>().camera_control_allowed = false;
    app.world_mut().resource_mut::<InputGateState>().editor_entry_allowed = false;
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::F12);
    app.update();
    assert!(!app.world().resource::<SceneEditorState>().open);

    app.world_mut().resource_mut::<InputGateState>().editor_entry_allowed = true;
    app.insert_resource(LoadingProgress::default());
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().reset_all();
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::F12);
    app.update();
    assert!(!app.world().resource::<SceneEditorState>().open); // 放行作者焦点不改变真实进图门。
}

#[test]
fn editor_world_transition_keeps_f12_open_and_unlocks_only_after_scene_startup_is_ready() {
    let mut app = keyboard_app(true);
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .scene_transition_in_progress = true;
    app.world_mut().resource_mut::<SceneEditorState>().open = true; // 切换场景不隐藏工作台。
    app.world_mut()
        .insert_resource(State::new(ScenePhase::Active));
    app.world_mut()
        .insert_resource(SceneStartupGate::waiting_for_collision(
            ScenePhase::Active,
        ));
    app.update();
    assert!(app.world().resource::<SceneEditorState>().open);
    assert!(
        app.world().resource::<SceneEditorState>().scene_transition_in_progress
    );
    app.world_mut().resource_mut::<SceneStartupGate>().ready = true;
    app.world_mut().insert_resource(LoadingProgress::default()); // 场景门已就绪但事务尚未释放时仍不能恢复场景操作。
    app.update();
    assert!(
        app.world().resource::<SceneEditorState>().scene_transition_in_progress
    );
    app.world_mut().remove_resource::<LoadingProgress>();
    app.update();
    let editor = app.world().resource::<SceneEditorState>();
    assert!(editor.open);
    assert!(!editor.scene_transition_in_progress);
    assert_eq!(editor.selected_scene_id, Some(SceneId::parse("MAP-editor-test").unwrap()));
}

#[test]
fn editor_world_failed_transition_back_to_main_keeps_workbench_and_clears_scene_lock() {
    let mut app = keyboard_app(false);
    app.world_mut().insert_resource(SceneEditorState {
        open: true,
        scene_transition_in_progress: true,
        selected_scene_id: Some(SceneId::parse("MAP-missing").unwrap()),
        ..default()
    });
    app.update(); // 缺失场景的公共失败路径已回到无加载资源的MainScene。
    let editor = app.world().resource::<SceneEditorState>();
    assert!(editor.open);
    assert!(!editor.scene_transition_in_progress);
    assert!(editor.selected_scene_id.is_none());
    assert!(editor.status.contains("目标场景不可用"));
}

fn close_app() -> App {
    let mut app = App::new();
    app.insert_resource(SceneEditorState {
        open: true,
        ..default()
    })
    .insert_resource(State::new(ScenePhase::Idle));
    init_close_resources(&mut app);
    app
}

fn init_close_resources(app: &mut App) {
    app.init_resource::<EditorAuthoringSession>()
    .init_resource::<TerrainBrushStroke>()
    .init_resource::<WaterBrushStroke>()
    .init_resource::<TerrainLiveApplyState>()
    .init_resource::<WaterPreviewState>()
    .init_resource::<WaterFileHandoff>()
    .init_resource::<BoundaryEditState>()
    .init_resource::<WaterNetworkEditState>()
    .init_resource::<SceneCreationTask>()
    .init_resource::<SceneDeletionTask>();
}

#[test]
fn editor_world_final_close_waits_for_loading_but_keeps_the_close_request() {
    let mut app = close_app();
    app.add_systems(Update, crate::ui::close::finish_scene_editor_close);
    for scene in [
        ScenePhase::Loading,
        ScenePhase::Active,
    ] {
        app.world_mut().insert_resource(State::new(scene));
        app.world_mut()
            .resource_mut::<SceneEditorState>()
            .request_close();
        app.update();
        let editor = app.world().resource::<SceneEditorState>();
        assert!(editor.open);
        assert!(editor.close_requested);
    }
    app.world_mut()
        .insert_resource(State::new(ScenePhase::Active));
    app.world_mut().insert_resource(ActiveDynamicScene { scene_id: Some(SceneId::parse("MAP-editor-test").unwrap()) });
    app.world_mut()
        .insert_resource(SceneStartupGate::waiting_for_collision(
            ScenePhase::Active,
        ));
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .request_close();
    app.update();
    assert!(app.world().resource::<SceneEditorState>().open);
    app.world_mut().resource_mut::<SceneStartupGate>().ready = true;
    app.update();
    assert!(!app.world().resource::<SceneEditorState>().open);
}

#[test]
fn editor_world_close_requested_during_transition_runs_after_full_scene_readiness() {
    let mut app = keyboard_app(true);
    init_close_resources(&mut app);
    app.add_systems(
        Update,
        crate::ui::close::finish_scene_editor_close
            .after(toggle_scene_editor)
            .before(sync_scene_editor_blocker),
    )
    .add_systems(
        Update,
        crate::ui::scene_loading::suppress_scene_loading_shortcuts.before(toggle_scene_editor),
    );
    app.update();
    app.world_mut().insert_resource(State::new(ScenePhase::Loading));
    app.world_mut().insert_resource(LoadingProgress::default());
    app.world_mut().resource_mut::<SceneEditorState>().scene_transition_in_progress = true;
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::F12);
    app.update();
    assert!(app.world().resource::<SceneEditorState>().close_requested);
    assert!(app.world().resource::<SceneEditorState>().open);
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().reset_all();
    app.world_mut().insert_resource(State::new(ScenePhase::Active));
    app.world_mut().insert_resource(SceneStartupGate::waiting_for_collision(ScenePhase::Active));
    app.update();
    assert!(app.world().resource::<SceneEditorState>().open);
    app.world_mut().resource_mut::<SceneStartupGate>().ready = true;
    app.world_mut().remove_resource::<LoadingProgress>();
    app.update(); // 正式进图解锁不能清除用户在交接期间提出的关闭意图。
    let editor = app.world().resource::<SceneEditorState>();
    assert!(!editor.open);
    assert!(!editor.close_requested);
    assert!(!editor.scene_transition_in_progress);
    assert!(!app.world().contains_resource::<EditorStartup>());
    assert!(!app.world().resource::<EditorInputState>().scene_editor_open);
}

#[test]
fn editor_world_f12_can_cancel_close_while_document_is_preparing() {
    let mut app = keyboard_app(true);
    app.init_resource::<crate::ui::scene_documents::SceneDocuments>()
        .add_systems(
            Update,
            crate::ui::scene_loading::suppress_scene_loading_shortcuts.before(toggle_scene_editor),
        );
    app.update();
    app.world_mut().resource_mut::<crate::ui::scene_documents::SceneDocuments>().preparing_target =
        Some(crate::ui::scene_documents::SceneDocumentSceneTarget {
            scene_id: SceneId::parse("MAP-preparing").unwrap(),
            scene: ScenePhase::Active,
            name: "切换中的场景".into(),
            spawn: Vec3::ZERO,
        });
    {
        let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
        editor.scene_transition_in_progress = true;
        editor.request_close();
    }
    {
        let mut keyboard = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keyboard.press(KeyCode::F12);
        keyboard.release(KeyCode::F12); // 同帧轻按也必须保留真实按下和释放状态。
        keyboard.press(KeyCode::KeyS);
    }
    app.update();
    let editor = app.world().resource::<SceneEditorState>();
    assert!(editor.open && editor.scene_transition_in_progress && !editor.close_requested);
    assert!(
        app.world()
            .resource::<crate::ui::scene_documents::SceneDocuments>()
            .preparing_target
            .is_some()
    );
    let keyboard = app.world().resource::<ButtonInput<KeyCode>>();
    assert!(!keyboard.just_pressed(KeyCode::KeyS));
    assert!(keyboard.pressed(KeyCode::KeyS));
    assert!(keyboard.just_pressed(KeyCode::F12) && keyboard.just_released(KeyCode::F12));
    assert!(!keyboard.pressed(KeyCode::F12));
}

#[test]
fn editor_world_empty_close_waits_for_save_and_preserves_authoring_state() {
    let mut app = close_app();
    app.insert_resource(EditorStartup)
        .add_systems(Update, crate::ui::close::finish_scene_editor_close);
    {
        let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
        editor.dirty = true;
        editor.request_close();
    }
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        session.revision = 19;
        session.saving = true;
    }
    app.update();
    assert!(app.world().resource::<SceneEditorState>().open);
    assert!(app.world().contains_resource::<EditorStartup>());
    app.world_mut().resource_mut::<EditorAuthoringSession>().saving = false;
    app.update();
    let editor = app.world().resource::<SceneEditorState>();
    assert!(!editor.open);
    assert!(editor.dirty); // 关闭工作台保留脏状态，不能伪造保存或放弃。
    assert!(!editor.save_requested);
    assert_eq!(app.world().resource::<EditorAuthoringSession>().revision, 19);
    assert!(app.world().contains_resource::<EditorStartup>());
    assert!(editor.empty_workspace_closed);
    assert_eq!(app.should_exit(), None);
}

#[test]
fn editor_world_empty_close_keeps_the_in_flight_scene_transition() {
    let mut app = close_app();
    app.insert_resource(EditorStartup)
        .add_systems(Update, crate::ui::close::finish_scene_editor_close);
    {
        let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
        editor.scene_transition_in_progress = true; // 草稿后台构造时场景可能仍是MainScene。
        editor.request_close();
    }
    app.update();
    let editor = app.world().resource::<SceneEditorState>();
    assert!(editor.open && editor.close_requested && editor.scene_transition_in_progress);
    assert!(app.world().contains_resource::<EditorStartup>());
    app.world_mut().resource_mut::<SceneEditorState>().scene_transition_in_progress = false;
    app.update(); // 模拟失败目标已经完整回到空工作台，再继续原关闭意图。
    assert!(!app.world().resource::<SceneEditorState>().open);
}

fn button_app() -> App {
    let mut app = close_app();
    app.init_resource::<EditorAssetCatalog>()
        .init_resource::<SceneBrowserState>()
        .init_resource::<TerrainPreviewState>()
        .init_resource::<SceneEditorNumericScrubMouseGuard>()
        .init_resource::<crate::ui::text_input::EditorTextInputGuard>()
        .init_resource::<DraftLoadTask>()
        .init_resource::<SceneFolderPersistence>()
        .init_resource::<Assets<SceneCatalogAsset>>()
        .init_resource::<bevy::input_focus::InputFocus>()
        .add_systems(Update, crate::ui::actions::handle_editor_action_buttons);
    app
}

#[test]
fn editor_world_hidden_button_press_is_not_replayed_after_reopening() {
    let mut app = button_app();
    app.world_mut().resource_mut::<SceneEditorState>().open = false;
    app.world_mut().spawn((feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed, SceneEditorActionButton(EditorAction::NewScene)));
    app.update(); // 已关闭的控件输入边沿必须被消费，不打开隐藏弹框。
    assert!(!app.world().resource::<SceneEditorState>().new_scene_dialog_open);
    app.world_mut().resource_mut::<SceneEditorState>().open = true;
    app.update(); // 原Pressed没有新的Changed边沿，重新打开不能迟到执行。
    assert!(!app.world().resource::<SceneEditorState>().new_scene_dialog_open);
    assert!(!app.world().resource::<SceneEditorState>().close_requested);
}

#[test]
fn editor_world_close_button_requests_f12_close_without_exiting_in_menu_or_scene() {
    let mut app = button_app();
    let button = app
        .world_mut()
        .spawn((
            feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed,
            SceneEditorActionButton(EditorAction::Close),
        ))
        .id();
    app.update();
    let editor = app.world().resource::<SceneEditorState>();
    assert!(editor.open);
    assert!(editor.close_requested);
    assert_eq!(app.should_exit(), None); // 关闭工作台不退出应用。
    app.world_mut()
        .insert_resource(State::new(ScenePhase::Active));
    app.world_mut().entity_mut(button).insert((UiInteraction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
    app.update();
    assert!(app.world().resource::<SceneEditorState>().close_requested);
    assert_eq!(app.should_exit(), None); // 进入场景后恢复原安全关闭流程，不意外退出游戏。
}

#[test]
fn direction_buttons_align_exactly_without_changing_projection_focus_or_zoom() {
    let mut app = button_app();
    let scene_id = ready_button_scene(&mut app);
    app.world_mut().resource_mut::<SceneEditorState>().dirty = true; // 导航也必须保留已有真实脏位。
    app.insert_resource(SceneEditorViewportState {
        _image: Handle::default(), camera: Entity::PLACEHOLDER,
        center: Vec3::new(10.0, 20.0, 30.0), scene_center: Vec3::ZERO,
        scene_half_extents: Vec3::new(500.0, 80.0, 300.0), extent: 120.0,
        yaw: 2.0, pitch: -0.5, framed_scene_id: Some(scene_id), render_size: UVec2::new(800, 600),
    });
    let button = app.world_mut().spawn((
        UiInteraction::None, SceneEditorActionButton(EditorAction::ViewTop),
    )).id();
    let revision = app.world().resource::<EditorAuthoringSession>().revision;
    for orthographic in [false, true] {
        app.world_mut().resource_mut::<SceneEditorState>().orthographic = orthographic;
        for (action, view, yaw, pitch) in [
            (EditorAction::ViewTop, crate::types::EditorView::Top, 0.0, std::f32::consts::FRAC_PI_2),
            (EditorAction::ViewFront, crate::types::EditorView::Front, 0.0, 0.0),
            (EditorAction::ViewSide, crate::types::EditorView::Side, std::f32::consts::FRAC_PI_2, 0.0),
            (EditorAction::ViewPerspective, crate::types::EditorView::Perspective, -0.72, 0.64),
        ] {
            app.world_mut().get_mut::<SceneEditorActionButton>(button).unwrap().0 = action;
            app.world_mut().entity_mut(button).insert((UiInteraction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
            app.update(); // 走真实按钮分派器，不能只在测试里直接设置方向资源。
            let editor = app.world().resource::<SceneEditorState>();
            let viewport = app.world().resource::<SceneEditorViewportState>();
            assert_eq!((editor.view, editor.orthographic), (view, orthographic));
            assert_eq!((viewport.yaw, viewport.pitch), (yaw, pitch));
            assert_eq!(viewport.center, Vec3::new(10.0, 20.0, 30.0));
            assert_eq!(viewport.extent, 120.0);
            assert!(editor.dirty);
            assert_eq!(app.world().resource::<EditorAuthoringSession>().revision, revision);
        }
    }
}

#[test]
fn orthographic_button_toggles_independently_and_highlights_beside_selected_direction() {
    let mut app = button_app();
    ready_button_scene(&mut app);
    app.world_mut().resource_mut::<SceneEditorState>().view = crate::types::EditorView::Top;
    app.add_systems(Update, crate::ui::actions::sync_editor_action_button_styles
            .after(crate::ui::actions::handle_editor_action_buttons));
    let top = app.world_mut().spawn((
        UiInteraction::None, SceneEditorActionButton(EditorAction::ViewTop), BackgroundColor::default(),
    )).id();
    let toggle = app.world_mut().spawn((
        feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed, SceneEditorActionButton(EditorAction::ToggleOrthographic), BackgroundColor::default(),
    )).id();
    app.update();
    assert!(app.world().resource::<SceneEditorState>().orthographic);
    for button in [top, toggle] {
        assert_eq!(app.world().get::<BackgroundColor>(button).unwrap().0, crate::ui::theme::SELECT_BG);
    }
    app.world_mut().entity_mut(toggle).insert((UiInteraction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
    app.update();
    let editor = app.world().resource::<SceneEditorState>();
    assert!(!editor.orthographic);
    assert_eq!(editor.view, crate::types::EditorView::Top);
    assert!(!editor.dirty);
    assert_eq!(app.world().get::<BackgroundColor>(top).unwrap().0, crate::ui::theme::SELECT_BG);
    assert_ne!(app.world().get::<BackgroundColor>(toggle).unwrap().0, crate::ui::theme::SELECT_BG);
}

fn ready_button_scene(app: &mut App) -> SceneId {
    let draft = crate::authoring::test_heightfield_draft(vec![0; 121].into());
    let scene_id = draft.scene_id.clone();
    app.insert_resource(State::new(ScenePhase::Active));
    app.insert_resource(ActiveDynamicScene { scene_id: Some(scene_id.clone()) });
    app.world_mut().resource_mut::<SceneEditorState>().selected_scene_id = Some(scene_id.clone());
    app.world_mut().resource_mut::<EditorAuthoringSession>().draft = Some(draft);
    app.insert_resource(SceneEditorViewportState {
        _image: Handle::default(), camera: Entity::PLACEHOLDER,
        center: Vec3::ZERO, scene_center: Vec3::ZERO, scene_half_extents: Vec3::ONE,
        extent: 50.0, yaw: 0.0, pitch: 0.5, framed_scene_id: Some(scene_id.clone()),
        render_size: UVec2::new(800, 600),
    }); // 导航正向回归必须具备当前草稿和正式视口采用身份。
    scene_id
}

#[test]
fn editor_world_controls_hide_only_scene_actions_and_leave_preset_workspaces_usable() {
    let mut app = App::new();
    app.insert_resource(State::new(ScenePhase::Idle))
        .init_resource::<VegetationPresetEditorState>()
        .init_resource::<SceneEditorState>()
        .add_systems(Update, sync_editor_world_controls);
    let close_label = app.world_mut().spawn(Text::new("启动")).id();
    let play_icon = app.world_mut().spawn((Text::new("▶"), TextColor(crate::ui::theme::TEXT_GREEN))).id();
    let close = app
        .world_mut()
        .spawn((
            Node {
                display: Display::None,
                ..default()
            },
            SceneEditorActionButton(EditorAction::Close),
        ))
        .add_child(play_icon)
        .add_child(close_label)
        .id();
    let new_scene = app
        .world_mut()
        .spawn((Node::default(), SceneEditorActionButton(EditorAction::NewScene)))
        .id();
    let undo = app
        .world_mut()
        .spawn((Node::default(), SceneEditorActionButton(EditorAction::Undo)))
        .id();
    let scene_tab = app
        .world_mut()
        .spawn((
            Node::default(),
            PresetEditorButton(PresetEditorAction::ActivateSceneTab),
        ))
        .id();
    let preset_save = app
        .world_mut()
        .spawn((
            Node::default(),
            PresetEditorButton(PresetEditorAction::Save),
        ))
        .id();
    let empty = app
        .world_mut()
        .spawn((Node::default(), SceneEditorEmptyWorkspace))
        .id();
    app.update();
    assert_eq!(
        app.world().get::<Node>(close).unwrap().display,
        Display::Flex
    );
    assert_eq!(app.world().get::<Text>(close_label).unwrap().0, "启动");
    assert_eq!(app.world().get::<Text>(play_icon).unwrap().0, "▶");
    assert_eq!(app.world().get::<TextColor>(play_icon).unwrap().0, crate::ui::theme::TEXT_GREEN);
    assert_eq!(
        app.world().get::<Node>(scene_tab).unwrap().display,
        Display::None
    );
    assert_eq!(
        app.world().get::<Node>(new_scene).unwrap().display,
        Display::Flex
    );
    assert_eq!(
        app.world().get::<Node>(preset_save).unwrap().display,
        Display::Flex
    );
    assert_eq!(
        app.world().get::<Node>(empty).unwrap().display,
        Display::Flex
    );
    app.world_mut()
        .resource_mut::<VegetationPresetEditorState>()
        .active_tab = PresetWorkspaceTab::Domain("fixture.domain", 0);
    app.update();
    assert_eq!(
        app.world().get::<Node>(empty).unwrap().display,
        Display::None
    );
    app.world_mut()
        .insert_resource(State::new(ScenePhase::Active));
    app.world_mut().insert_resource(ActiveDynamicScene { scene_id: Some(SceneId::parse("MAP-editor-test").unwrap()) });
    app.update();
    assert_eq!(
        app.world().get::<Node>(close).unwrap().display,
        Display::Flex
    );
    assert_eq!(app.world().get::<Text>(close_label).unwrap().0, "启动");
    assert_eq!(app.world().get::<Text>(play_icon).unwrap().0, "▶");
    assert_eq!(
        app.world().get::<Node>(scene_tab).unwrap().display,
        Display::Flex
    );
    assert_eq!(
        app.world().get::<Node>(undo).unwrap().display,
        Display::None
    );
    app.world_mut()
        .resource_mut::<VegetationPresetEditorState>()
        .active_tab = PresetWorkspaceTab::Scene;
    app.update();
    assert_eq!(
        app.world().get::<Node>(undo).unwrap().display,
        Display::Flex
    );
}

#[test]
fn editor_world_viewport_allocates_only_after_scene_entry_and_releases_it_on_exit() {
    let mut app = App::new();
    // 正式视口创建鼠标指针，最小夹具要先提供拾取系统的指针索引。
    app.init_resource::<bevy::picking::pointer::PointerMap>()
        .init_resource::<Assets<Image>>()
        .init_resource::<SceneEditorState>()
        .init_resource::<crate::resources::SceneEditorViewMemory>()
        .insert_resource(State::new(ScenePhase::Idle))
        .add_systems(Update, crate::viewport::setup_editor_viewport);
    let node = app.world_mut().spawn(SceneEditorViewport).id();
    app.update();
    assert!(!app.world().contains_resource::<SceneEditorViewportState>());
    assert_eq!(app.world().resource::<Assets<Image>>().len(), 0);
    assert_eq!(
        app.world_mut()
            .query_filtered::<Entity, With<SceneEditorCamera>>()
            .iter(app.world())
            .count(),
        0
    );
    app.world_mut()
        .insert_resource(State::new(ScenePhase::Active));
    app.world_mut().insert_resource(ActiveDynamicScene { scene_id: Some(SceneId::parse("MAP-editor-test").unwrap()) });
    app.world_mut()
        .insert_resource(SceneStartupGate::waiting_for_collision(
            ScenePhase::Active,
        ));
    app.update();
    assert!(!app.world().contains_resource::<SceneEditorViewportState>());
    app.world_mut().resource_mut::<SceneStartupGate>().ready = true;
    app.update(); // BSN节点的 Added 已经过期，真实进图后仍应创建场景 RTT。
    let camera = app.world().resource::<SceneEditorViewportState>().camera;
    assert!(app.world().get::<SceneEditorCamera>(camera).is_some());
    assert!(app.world().get::<bevy::camera::Hdr>(camera).is_some()); // 四视图由工作台显式拥有HDR，不能依赖动态大气临时引入。
    assert!(app.world().get::<ViewportNode>(node).is_some());
    assert_eq!(app.world().resource::<Assets<Image>>().len(), 1);
    app.update();
    assert_eq!(
        app.world().resource::<SceneEditorViewportState>().camera,
        camera
    );
    assert_eq!(app.world().resource::<Assets<Image>>().len(), 1);
    app.world_mut()
        .insert_resource(State::new(ScenePhase::Loading));
    app.update();
    assert!(!app.world().contains_resource::<SceneEditorViewportState>());
    assert!(app.world().get_entity(camera).is_err());
    assert!(app.world().get::<ViewportNode>(node).is_none());
    assert_eq!(app.world().resource::<Assets<Image>>().len(), 0);
    app.world_mut().remove_resource::<ActiveDynamicScene>(); // 模拟下一个场景身份尚未采用。
    app.world_mut()
        .insert_resource(State::new(ScenePhase::Active));
    app.update();
    assert!(!app.world().contains_resource::<SceneEditorViewportState>());
    app.world_mut().insert_resource(ActiveDynamicScene {
        scene_id: Some(SceneId::parse("MAP-editor-test").unwrap()),
    });
    app.update();
    assert!(app.world().contains_resource::<SceneEditorViewportState>());
}
