//! 用生产资格、按钮样式和两种输入验证禁用及恢复，不启动作者IO或改写配置。

use feathrise_editor_ui::UiInteraction;
use super::{EditorActionAvailability, EditorActionInputs};
use crate::authoring::{
    DraftLoadTask, EditorAuthoringSession, EditorCommand, HeightChange, TerrainPreviewState,
    WaterBrushStroke, WaterNetworkEditState,
};
use crate::components::{SceneEditorActionButton, SceneEditorSettingsButton};
use crate::resources::*;
use crate::types::{EditorAction, EditorSettingsAction, EditorTool, EditorViewportFraming};
use crate::ui::actions::{
    handle_editor_action_buttons, handle_editor_shortcuts, sync_editor_action_button_styles,
};
use crate::ui::theme as colors;
use crate::ui::vegetation_preset_editor::{PresetWorkspaceTab, VegetationPresetEditorState};
use bevy::ecs::system::RunSystemOnce;
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;
use feathrise_editor_ui::ActiveNumericInput;
use feathrise_scene_content::{SceneCatalogAsset, SceneId};
use feathrise_editor_core::EditorSettings;
use feathrise_editor_core::EditorKeyBinding;
use feathrise_editor_core::EditorShortcutAction;

const SCENE_ACTIONS: [EditorAction; 10] = [
    EditorAction::Save,
    EditorAction::Undo,
    EditorAction::Redo,
    EditorAction::ViewPerspective,
    EditorAction::ViewTop,
    EditorAction::ViewFront,
    EditorAction::ViewSide,
    EditorAction::ToggleOrthographic,
    EditorAction::FocusSelected,
    EditorAction::FrameScene,
];

fn app(ready: bool) -> App {
    let mut app = App::new();
    let draft = crate::authoring::test_heightfield_draft(vec![0; 121].into());
    let scene_id = draft.scene_id.clone();
    app.insert_resource(State::new(if ready {
        feathrise_scene_core::state::ScenePhase::Active
    } else {
        feathrise_scene_core::state::ScenePhase::Idle
    }))
    .insert_resource(feathrise_scene_content::ActiveDynamicScene { scene_id: Some(scene_id.clone()) })
    .insert_resource(SceneEditorState {
        open: true,
        selected_scene_id: Some(scene_id.clone()),
        ..default()
    })
    .insert_resource(EditorAuthoringSession {
        draft: Some(draft),
        ..default()
    })
    .insert_resource(SceneEditorViewportState {
        _image: Handle::default(),
        camera: Entity::PLACEHOLDER,
        center: Vec3::ZERO,
        scene_center: Vec3::ZERO,
        scene_half_extents: Vec3::ONE,
        extent: 50.0,
        yaw: 0.0,
        pitch: 0.5,
        framed_scene_id: Some(scene_id),
        render_size: UVec2::new(800, 600),
    })
    .init_resource::<ButtonInput<KeyCode>>()
    .init_resource::<ButtonInput<MouseButton>>()
    .init_resource::<ActiveNumericInput>()
    .init_resource::<EditorSettings>()
    .init_resource::<InputFocus>()
    .init_resource::<SceneEditorSettingsState>()
    .init_resource::<SceneEditorNumericScrubMouseGuard>()
    .init_resource::<SceneDeletionTask>()
    .init_resource::<SceneCreationTask>()
    .init_resource::<EditorAssetCatalog>()
    .init_resource::<SceneBrowserState>()
    .init_resource::<SceneFolderPersistence>()
    .init_resource::<Assets<SceneCatalogAsset>>()
    .init_resource::<DraftLoadTask>()
    .init_resource::<TerrainPreviewState>()
    .init_resource::<WaterBrushStroke>()
    .init_resource::<WaterNetworkEditState>()
    .init_resource::<crate::ui::text_input::EditorTextInputGuard>()
    .init_resource::<VegetationPresetEditorState>()
    .init_resource::<crate::ui::surface_preset_editor::SurfacePresetEditorState>()
    .init_resource::<crate::ui::water_preset_editor::WaterPresetEditorState>()
    .init_resource::<crate::ui::building_presets::BuildingPresetEditorState>()
    .init_resource::<crate::game_authoring::scene_interaction::SceneInteractionState>()
    .init_resource::<crate::viewport::LayerCandidateState>()
    .add_systems(
        Update,
        (
            handle_editor_action_buttons,
            handle_editor_shortcuts,
            sync_editor_action_button_styles,
        )
            .chain(),
    );
    app
}

fn button(app: &mut App, action: EditorAction) -> Entity {
    let parent = app
        .world_mut()
        .spawn((
            SceneEditorActionButton(action),
            UiInteraction::None,
            BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL),
            Node {
                width: Val::Px(80.0),
                height: Val::Px(26.0),
                ..default()
            },
        ))
        .id();
    app.world_mut().spawn((
        Text::new("按钮"),
        TextColor(colors::TEXT_PRIMARY),
        ChildOf(parent),
    ));
    parent
}

fn availability(app: &mut App) -> EditorActionAvailability {
    app.world_mut()
        .run_system_once(
            |inputs: EditorActionInputs,
             editor: Res<SceneEditorState>,
             viewport: Res<SceneEditorViewportState>| {
                inputs.snapshot(&editor, Some(&viewport))
            },
        )
        .unwrap()
}

fn click(app: &mut App, button: Entity) {
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .reset_all();
    app.world_mut().entity_mut(button).insert((UiInteraction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
    app.update();
}

fn press(app: &mut App, keys: &[KeyCode]) {
    let mut input = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    input.reset_all();
    for key in keys {
        input.press(*key);
    }
    drop(input);
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .clear();
}

fn clear_requests(app: &mut App) {
    let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
    editor.save_requested = false;
    editor.undo_requested = false;
    editor.redo_requested = false;
    editor.viewport_framing_requested = None;
}

fn assert_no_requests(app: &App) {
    let editor = app.world().resource::<SceneEditorState>();
    assert!(!editor.save_requested && !editor.undo_requested && !editor.redo_requested);
    assert!(editor.viewport_framing_requested.is_none());
}

fn assert_style(app: &mut App, button: Entity, reason: Option<&str>) {
    let action = app.world().get::<SceneEditorActionButton>(button).unwrap().0;
    assert_eq!(availability(app).reason(action), reason);
    assert_eq!(
        app.world().get::<InteractionDisabled>(button).is_some(),
        reason.is_some()
    );
    let child = app.world().get::<Children>(button).unwrap()[0];
    assert_eq!(
        app.world().get::<TextColor>(child).unwrap().0,
        if reason.is_some() {
            colors::TEXT_MUTED
        } else {
            colors::TEXT_PRIMARY
        }
    );
    if reason.is_some() {
        assert_eq!(
            app.world().get::<BackgroundColor>(button).unwrap().0,
            colors::SLOT_BG_DISABLED
        );
    }
}

fn history() -> EditorCommand {
    EditorCommand::TerrainChanged(vec![HeightChange {
        index: 0,
        before: 0.0,
        after: 1.0,
    }])
}

#[test]
fn menu_with_catalog_identity_and_draft_is_not_a_ready_scene() {
    let mut app = app(false);
    let buttons: Vec<_> = SCENE_ACTIONS
        .into_iter()
        .map(|action| button(&mut app, action))
        .collect();
    let new_scene = button(&mut app, EditorAction::NewScene);
    let settings = app
        .world_mut()
        .spawn((
            SceneEditorSettingsButton(EditorSettingsAction::Open),
            UiInteraction::None,
        ))
        .id();
    app.update();
    for button in buttons {
        assert_style(&mut app, button, Some("尚未进入就绪场景"));
        click(&mut app, button);
        assert_no_requests(&app);
    }
    for keys in [
        vec![KeyCode::ControlLeft, KeyCode::KeyS],
        vec![KeyCode::ControlLeft, KeyCode::KeyZ],
        vec![KeyCode::ControlLeft, KeyCode::KeyY],
        vec![KeyCode::KeyF],
        vec![KeyCode::Home],
    ] {
        press(&mut app, &keys);
        assert_no_requests(&app);
    }
    assert!(!app.world().resource::<SceneEditorState>().orthographic);
    assert!(app.world().get::<InteractionDisabled>(new_scene).is_none());
    assert!(app.world().get::<InteractionDisabled>(settings).is_none());
    app.world_mut()
        .resource_mut::<SceneBrowserState>()
        .write_blocked = None; // 归类首载完成是新建自己的原有门，不应误算为缺少场景。
    click(&mut app, new_scene);
    assert!(app.world().resource::<SceneEditorState>().new_scene_dialog_open);
}

#[test]
fn empty_histories_and_saving_block_both_inputs_then_restore_without_rebuilding() {
    for (action, shortcut, reason) in [
        (
            EditorAction::Undo,
            EditorShortcutAction::Undo,
            "当前场景没有可撤销操作",
        ),
        (
            EditorAction::Redo,
            EditorShortcutAction::Redo,
            "当前场景没有可重做操作",
        ),
    ] {
        let mut app = app(true);
        app.world_mut()
            .resource_mut::<EditorSettings>()
            .editor_keybindings
            .assign(shortcut, Some(EditorKeyBinding::plain(KeyCode::KeyK)))
            .unwrap();
        let button = button(&mut app, action);
        let node = app.world().get::<Node>(button).unwrap().clone();
        app.update();
        assert_style(&mut app, button, Some(reason));
        click(&mut app, button);
        press(&mut app, &[KeyCode::KeyK]);
        assert_no_requests(&app);
        {
            let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
            session.undo.push(history());
            session.redo.push(history());
            session.saving = true;
        }
        click(&mut app, button);
        press(&mut app, &[KeyCode::KeyK]);
        assert_style(&mut app, button, Some("当前场景正在保存"));
        assert_no_requests(&app);
        app.world_mut()
            .resource_mut::<EditorAuthoringSession>()
            .saving = false;
        // 不先运行表现：上一帧的禁用标记不能阻止本帧已经恢复的合法点击。
        click(&mut app, button);
        assert_style(&mut app, button, None);
        let editor = app.world().resource::<SceneEditorState>();
        assert!(if action == EditorAction::Undo {
            editor.undo_requested
        } else {
            editor.redo_requested
        });
        clear_requests(&mut app);
        press(&mut app, &[KeyCode::KeyK]);
        let editor = app.world().resource::<SceneEditorState>();
        assert!(if action == EditorAction::Undo {
            editor.undo_requested
        } else {
            editor.redo_requested
        });
        assert_eq!(app.world().get::<Node>(button).unwrap(), &node);
    }
}

#[test]
fn live_save_state_overrules_both_stale_enabled_and_stale_disabled_markers() {
    let mut app = app(true);
    let save = button(&mut app, EditorAction::Save);
    app.world_mut()
        .resource_mut::<EditorSettings>()
        .editor_keybindings
        .assign(
            EditorShortcutAction::Save,
            Some(EditorKeyBinding::plain(KeyCode::KeyK)),
        )
        .unwrap();
    app.update();
    assert_style(&mut app, save, None);
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .saving = true;
    click(&mut app, save);
    press(&mut app, &[KeyCode::KeyK]);
    assert_no_requests(&app);
    assert_style(&mut app, save, Some("当前场景正在保存"));
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .saving = false;
    click(&mut app, save);
    assert!(app.world().resource::<SceneEditorState>().save_requested);
    assert_style(&mut app, save, None);
    clear_requests(&mut app);
    press(&mut app, &[KeyCode::KeyK]);
    assert!(app.world().resource::<SceneEditorState>().save_requested);
}

#[test]
fn terrain_save_restores_only_after_exact_scene_and_revision_are_complete() {
    let mut app = app(true);
    let save = button(&mut app, EditorAction::Save);
    let scene_id = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        session.terrain_revision = 7;
        let draft = session.draft.as_mut().unwrap();
        draft.terrain_dirty = true;
        draft.scene_id.clone()
    };
    app.update();
    assert_style(&mut app, save, Some("地形高度、法线与碰撞仍在后台生成"));
    for (applied_scene_id, revision) in [(SceneId::parse("MAP-other").unwrap(), 7), (scene_id.clone(), 6)]
    {
        {
            let mut preview = app.world_mut().resource_mut::<TerrainPreviewState>();
            preview.applied_scene_id = Some(applied_scene_id);
            preview.applied_revision = revision;
            preview.mesh = Some(Handle::default());
            preview.height_image = Some(Handle::default());
        }
        click(&mut app, save);
        press(&mut app, &[KeyCode::ControlLeft, KeyCode::KeyS]);
        assert_no_requests(&app);
    }
    {
        let mut preview = app.world_mut().resource_mut::<TerrainPreviewState>();
        preview.applied_scene_id = Some(scene_id);
        preview.applied_revision = 7;
    }
    click(&mut app, save);
    assert_style(&mut app, save, None);
    assert!(app.world().resource::<SceneEditorState>().save_requested);
    clear_requests(&mut app);
    press(&mut app, &[KeyCode::ControlLeft, KeyCode::KeyS]);
    assert!(app.world().resource::<SceneEditorState>().save_requested);
}

#[test]
fn focus_requires_confirmed_selection_and_the_current_viewport_identity() {
    let mut app = app(true);
    let focus = button(&mut app, EditorAction::FocusSelected);
    app.world_mut()
        .resource_mut::<EditorSettings>()
        .editor_keybindings
        .assign(
            EditorShortcutAction::FocusSelected,
            Some(EditorKeyBinding::plain(KeyCode::KeyG)),
        )
        .unwrap();
    app.world_mut()
        .resource_mut::<crate::viewport::LayerCandidateState>()
        .hover = Some(crate::viewport::LayerTarget::Portal("hover-only".into()));
    app.update();
    assert_style(&mut app, focus, Some("请先选择可聚焦的资产、水体或传送点"));
    click(&mut app, focus);
    press(&mut app, &[KeyCode::KeyG]);
    assert_no_requests(&app);
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        draft
            .portals
            .portals
            .push(feathrise_scene_content::ScenePortalRecord {
                terrain_binding: None,
                arrival_terrain_binding: None,
                id: "confirmed".into(),
                label: "入口".into(),
                label_customized: true,
                position: [0.0; 3],
                arrival: [1.0, 0.0, 1.0],
                target: feathrise_scene_content::ScenePortalTarget {
                    scene_id: draft.scene_id.clone(),
                    portal_id: None,
                },
                color_srgba: [1.0; 4],
                arrow_degrees: 0.0,
            });
    }
    {
        let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
        editor.tool = EditorTool::Portal;
        editor.selected_current_portal_id = Some("confirmed".into());
    }
    click(&mut app, focus);
    assert_style(&mut app, focus, None);
    assert_eq!(
        app.world()
            .resource::<SceneEditorState>()
            .viewport_framing_requested,
        Some(EditorViewportFraming::Selected)
    );
    clear_requests(&mut app);
    press(&mut app, &[KeyCode::KeyF]);
    assert_no_requests(&app); // 已改键不能继续使用默认F。
    press(&mut app, &[KeyCode::KeyG]);
    assert_eq!(
        app.world()
            .resource::<SceneEditorState>()
            .viewport_framing_requested,
        Some(EditorViewportFraming::Selected)
    );
    clear_requests(&mut app);
    app.world_mut()
        .resource_mut::<SceneEditorViewportState>()
        .framed_scene_id = Some(SceneId::parse("MAP-other").unwrap());
    click(&mut app, focus);
    press(&mut app, &[KeyCode::KeyG, KeyCode::Home]);
    assert_no_requests(&app);
    assert_style(&mut app, focus, Some("场景视口尚未就绪"));
}

#[test]
fn modal_workspace_and_scene_identity_changes_do_not_queue_delayed_commands() {
    for case in 0..6 {
        let mut app = app(true);
        let save = button(&mut app, EditorAction::Save);
        app.update();
        match case {
            0 => {
                app.world_mut()
                    .resource_mut::<SceneEditorState>()
                    .new_scene_dialog_open = true
            }
            1 => {
                app.world_mut()
                    .resource_mut::<SceneEditorState>()
                    .scene_transition_in_progress = true
            }
            2 => {
                app.world_mut()
                    .resource_mut::<SceneEditorSettingsState>()
                    .open = true
            }
            3 => app
                .world_mut()
                .resource_mut::<SceneEditorNumericScrubMouseGuard>()
                .advance(true, false, false),
            4 => {
                app.world_mut()
                    .resource_mut::<VegetationPresetEditorState>()
                    .active_tab = PresetWorkspaceTab::Document
            }
            _ => {
                app.world_mut()
                    .resource_mut::<SceneEditorState>()
                    .selected_scene_id = Some(SceneId::parse("MAP-other").unwrap())
            }
        }
        click(&mut app, save);
        press(&mut app, &[KeyCode::ControlLeft, KeyCode::KeyS]);
        assert_no_requests(&app);
        assert!(app.world().get::<InteractionDisabled>(save).is_some());
        let scene_id = app
            .world()
            .resource::<EditorAuthoringSession>()
            .draft
            .as_ref()
            .map(|draft| draft.scene_id.clone());
        {
            let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
            editor.new_scene_dialog_open = false;
            editor.scene_transition_in_progress = false;
            editor.selected_scene_id = scene_id;
        }
        *app.world_mut()
            .resource_mut::<SceneEditorNumericScrubMouseGuard>() = default();
        app.world_mut()
            .resource_mut::<SceneEditorSettingsState>()
            .open = false;
        app.world_mut()
            .resource_mut::<VegetationPresetEditorState>()
            .active_tab = PresetWorkspaceTab::Scene;
        app.update();
        assert_no_requests(&app); // 被丢弃的Pressed边沿不能在恢复时重放。
        assert_style(&mut app, save, None);
        click(&mut app, save);
        assert!(app.world().resource::<SceneEditorState>().save_requested);
    }
}

#[test]
fn deletion_confirmation_holds_all_scene_commands_until_the_transaction_releases() {
    let mut app = app(true);
    let save = button(&mut app, EditorAction::Save);
    let scene_id = app
        .world()
        .resource::<SceneEditorState>()
        .selected_scene_id
        .clone()
        .unwrap();
    app.world_mut()
        .resource_mut::<SceneDeletionTask>()
        .confirmation = Some(crate::types::SceneDeletionRequest {
        folder: format!("scenes/{}", scene_id.as_str()),
        scene_id,
        display_name: "待回收场景".into(),
    });
    app.update();
    for action in SCENE_ACTIONS {
        assert_eq!(
            availability(&mut app).reason(action),
            Some("场景回收事务尚未完成")
        );
    }
    click(&mut app, save);
    press(&mut app, &[KeyCode::ControlLeft, KeyCode::KeyS]);
    assert_no_requests(&app);
    app.world_mut()
        .resource_mut::<SceneDeletionTask>()
        .cancel_confirmation();
    click(&mut app, save);
    assert!(app.world().resource::<SceneEditorState>().save_requested);
    assert_style(&mut app, save, None);
}

#[test]
fn framing_without_container_bounds_is_independent_of_history_and_save_gates() {
    let state = EditorActionAvailability {
        current_scene: true,
        viewport_ready: true,
        undo_empty: true,
        redo_empty: true,
        terrain_pending: true,
        ..default()
    };
    assert_eq!(state.reason(EditorAction::FrameScene), None); // 空容器也能恢复默认取景。
    assert_eq!(state.reason(EditorAction::ViewTop), None);
    assert_eq!(state.reason(EditorAction::NewScene), None);
    assert_eq!(state.reason(EditorAction::Close), None);
    assert_eq!(state.reason(EditorAction::ToolRaise), None);
    assert_eq!(
        EditorActionAvailability::shortcut_action(EditorShortcutAction::MoveSelected),
        None
    );
}

#[test]
fn mesh_edit_keeps_confirmed_save_available_and_disables_object_commands_in_place() {
    use crate::authoring::mesh_edit::MeshEditState;
    let mut app = app(true);
    app.init_resource::<MeshEditState>();
    let save = button(&mut app, EditorAction::Save);
    let framing = button(&mut app, EditorAction::FrameScene);
    let node = app.world().get::<Node>(framing).unwrap().clone();
    app.world_mut().resource_mut::<MeshEditState>().activate_for_test();
    app.update();
    assert_style(&mut app, save, None);
    assert_style(&mut app, framing, Some("请先按 Tab 返回对象模式"));
    click(&mut app, framing);
    assert_no_requests(&app);
    click(&mut app, save);
    assert!(app.world().resource::<SceneEditorState>().save_requested);
    assert_eq!(app.world().get::<Node>(framing).unwrap(), &node, "禁用只改变资格与表现，不重建输入身份");
}
