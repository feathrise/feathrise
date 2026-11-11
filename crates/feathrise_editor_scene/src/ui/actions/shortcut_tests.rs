//! 场景快捷键的真实分派回归：改键生效、裸键精确匹配及中央文档/文本/拖动隔离。

use super::*;
use crate::types::EditorViewportFraming;
use crate::ui::vegetation_preset_editor::PresetWorkspaceTab;
use feathrise_editor_core::EditorKeyBinding;

#[test]
fn snap_shortcut_is_configurable_and_ctrl_y_remains_redo() {
    let mut app = app();
    app.world_mut().resource_mut::<EditorAuthoringSession>().redo.push(crate::authoring::EditorCommand::TerrainChanged(vec![crate::authoring::HeightChange { index: 0, before: 0.0, after: 1.0 }]));
    press(&mut app, &[KeyCode::KeyY]);
    assert!(app.world().resource::<EditorSettings>().editor_snapping.enabled);
    press(&mut app, &[KeyCode::ControlLeft, KeyCode::KeyY]);
    assert!(app.world().resource::<EditorSettings>().editor_snapping.enabled);
    assert!(app.world().resource::<SceneEditorState>().redo_requested);
    app.world_mut().resource_mut::<EditorSettings>().editor_keybindings.assign(EditorShortcutAction::ToggleSnapping, Some(EditorKeyBinding::plain(KeyCode::KeyK))).unwrap();
    press(&mut app, &[KeyCode::KeyY]);
    assert!(app.world().resource::<EditorSettings>().editor_snapping.enabled);
    press(&mut app, &[KeyCode::KeyK]);
    assert!(!app.world().resource::<EditorSettings>().editor_snapping.enabled);
}

fn app() -> App {
    let mut app = App::new();
    let draft = crate::authoring::test_heightfield_draft(vec![0; 121].into());
    let scene_id = draft.scene_id.clone();
    app.insert_resource(State::new(feathrise_scene_core::state::ScenePhase::Active))
        .insert_resource(feathrise_scene_content::ActiveDynamicScene { scene_id: Some(scene_id.clone()) })
        .insert_resource(EditorAuthoringSession { draft: Some(draft), ..default() })
        .insert_resource(SceneEditorViewportState {
            _image: Handle::default(), camera: Entity::PLACEHOLDER,
            center: Vec3::ZERO, scene_center: Vec3::ZERO, scene_half_extents: Vec3::ONE,
            extent: 50.0, yaw: 0.0, pitch: 0.5, framed_scene_id: Some(scene_id.clone()),
            render_size: UVec2::new(800, 600),
        }); // 正向快捷键用真实就绪草稿和已采用视口，不再靠打开F12冒充进入场景。
    app.insert_resource(SceneEditorState {
        open: true,
        selected_scene_id: Some(scene_id),
        ..default()
    })
    .init_resource::<ButtonInput<KeyCode>>()
    .init_resource::<ButtonInput<MouseButton>>()
    .init_resource::<ActiveNumericInput>()
    .init_resource::<SceneEditorNumericScrubMouseGuard>()
    .init_resource::<SceneDeletionTask>()
    .init_resource::<WaterNetworkEditState>()
    .init_resource::<VegetationPresetEditorState>()
    .init_resource::<SurfacePresetEditorState>()
    .init_resource::<WaterPresetEditorState>()
    .init_resource::<super::super::building_presets::BuildingPresetEditorState>()
    .init_resource::<crate::game_authoring::scene_interaction::SceneInteractionState>()
    .init_resource::<InputFocus>()
    .init_resource::<super::super::text_input::EditorTextInputGuard>()
    .init_resource::<EditorSettings>()
    .init_resource::<crate::authoring::basic_shapes::BasicShapeState>()
    .add_systems(Update, handle_editor_shortcuts);
    app
}

#[test]
fn add_menu_shortcut_uses_the_shared_text_modal_and_drag_ownership_gates() {
    use crate::authoring::basic_shapes::BasicShapeState;
    let mut fixture = app();
    press(&mut fixture, &[KeyCode::ShiftRight, KeyCode::KeyA]);
    assert!(fixture.world().resource::<BasicShapeState>().request_menu);
    fixture.world_mut().resource_mut::<EditorSettings>().editor_keybindings.assign(
        EditorShortcutAction::AddObjectMenu,
        Some(EditorKeyBinding { control: true, ..EditorKeyBinding::plain(KeyCode::KeyA) }),
    ).unwrap();
    fixture.world_mut().resource_mut::<BasicShapeState>().request_menu = false;
    press(&mut fixture, &[KeyCode::ShiftLeft, KeyCode::KeyA]);
    assert!(!fixture.world().resource::<BasicShapeState>().request_menu);
    press(&mut fixture, &[KeyCode::ControlLeft, KeyCode::KeyA]);
    assert!(fixture.world().resource::<BasicShapeState>().request_menu);
    for case in 0..5 {
        let mut app = app();
        match case {
            0 => app.world_mut().resource_mut::<ActiveNumericInput>().entity = Some(Entity::PLACEHOLDER),
            1 => app.world_mut().resource_mut::<super::super::text_input::EditorTextInputGuard>().block_shortcuts = true,
            2 => { let entity = app.world_mut().spawn(bevy::text::EditableText::default()).id(); app.world_mut().resource_mut::<InputFocus>().set(entity, bevy::input_focus::FocusCause::Navigated); },
            3 => app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left),
            _ => app.world_mut().resource_mut::<SceneEditorState>().new_scene_dialog_open = true,
        }
        press(&mut app, &[KeyCode::ShiftLeft, KeyCode::KeyA]);
        assert!(!app.world().resource::<BasicShapeState>().request_menu, "添加快捷键隔离场景{case}");
    }
}

#[test]
fn add_mesh_shortcut_opens_from_other_central_tabs_without_changing_them() {
    for tab in [PresetWorkspaceTab::Document, PresetWorkspaceTab::AssetPreview] {
        let mut app = app();
        app.world_mut().resource_mut::<VegetationPresetEditorState>().active_tab = tab;
        press(&mut app, &[KeyCode::ShiftLeft, KeyCode::KeyA]);
        assert!(app.world().resource::<crate::authoring::basic_shapes::BasicShapeState>().request_menu);
        assert_eq!(app.world().resource::<VegetationPresetEditorState>().active_tab, tab);
    }
}

#[test]
fn mesh_edit_blocks_object_shortcuts_but_saves_and_undoes_confirmed_geometry() {
    use crate::authoring::mesh_edit::MeshEditState;
    let mut app = app();
    app.init_resource::<crate::viewport::modes::ViewportModeState>()
        .init_resource::<MeshEditState>();
    app.world_mut().resource_mut::<MeshEditState>().activate_for_test();
    for keys in [vec![KeyCode::KeyW], vec![KeyCode::KeyE], vec![KeyCode::KeyR], vec![KeyCode::ShiftLeft, KeyCode::KeyA]] {
        press(&mut app, &keys);
        assert!(!app.world().resource::<SceneEditorState>().placement_transform_enabled);
        assert!(!app.world().resource::<crate::authoring::basic_shapes::BasicShapeState>().request_menu);
    }
    press(&mut app, &[KeyCode::ControlLeft, KeyCode::KeyS]);
    assert!(app.world().resource::<SceneEditorState>().save_requested, "已确认几何复用原场景保存");
    app.world_mut().resource_mut::<EditorAuthoringSession>().undo.push(crate::authoring::EditorCommand::TerrainChanged(vec![crate::authoring::HeightChange { index: 0, before: 0.0, after: 1.0 }]));
    press(&mut app, &[KeyCode::ControlLeft, KeyCode::KeyZ]);
    assert!(app.world().resource::<SceneEditorState>().undo_requested, "历史变更由核心身份核对结束旧编辑目标");
}

fn select_portal(app: &mut App) {
    let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
    let draft = session.draft.as_mut().unwrap();
    draft.portals.portals.push(feathrise_scene_content::ScenePortalRecord {
        terrain_binding: None,
        arrival_terrain_binding: None,
        id: "focus-portal".into(), label: "入口".into(), label_customized: true,
        position: [0.0; 3], arrival: [1.0, 0.0, 1.0],
        target: feathrise_scene_content::ScenePortalTarget { scene_id: draft.scene_id.clone(), portal_id: None },
        color_srgba: [1.0; 4], arrow_degrees: 0.0,
    });
    drop(session);
    let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
    editor.tool = EditorTool::Portal;
    editor.selected_current_portal_id = Some("focus-portal".into());
}

fn press(app: &mut App, keys: &[KeyCode]) {
    let mut input = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    input.reset_all();
    for &key in keys {
        input.press(key);
    }
    drop(input);
    app.update();
}

#[test]
fn rotation_scale_and_move_shortcuts_share_one_mode_and_respect_input_owners() {
    use crate::viewport::modes::{ViewportMode, ViewportModeState};
    let mut app = app();
    app.init_resource::<ViewportModeState>();
    for (key, expected) in [(KeyCode::KeyE, ViewportMode::Rotate), (KeyCode::KeyR, ViewportMode::Scale), (KeyCode::KeyW, ViewportMode::Move), (KeyCode::KeyE, ViewportMode::Rotate), (KeyCode::KeyE, ViewportMode::Select)] {
        press(&mut app, &[key]);
        assert_eq!(app.world().resource::<ViewportModeState>().mode, expected);
        assert_eq!(app.world().resource::<SceneEditorState>().placement_transform_enabled, expected.transforms_placement());
        assert!(!app.world().resource::<SceneEditorState>().dirty);
    }
    app.world_mut().resource_mut::<EditorSettings>().editor_keybindings.assign(EditorShortcutAction::RotateSelected, Some(EditorKeyBinding { control: true, ..EditorKeyBinding::plain(KeyCode::KeyT) })).unwrap();
    press(&mut app, &[KeyCode::KeyE]);
    assert_eq!(app.world().resource::<ViewportModeState>().mode, ViewportMode::Select);
    press(&mut app, &[KeyCode::ControlRight, KeyCode::KeyT]);
    assert_eq!(app.world().resource::<ViewportModeState>().mode, ViewportMode::Rotate);
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
    press(&mut app, &[KeyCode::KeyR]);
    assert_eq!(app.world().resource::<ViewportModeState>().mode, ViewportMode::Rotate);
}

#[test]
fn move_shortcut_and_custom_binding_toggle_tool_and_explicit_mode() {
    let mut app = app();
    app.init_resource::<crate::viewport::modes::ViewportModeState>();
    press(&mut app, &[KeyCode::KeyW]);
    assert!(app.world().resource::<SceneEditorState>().placement_transform_enabled);
    assert_eq!(app.world().resource::<SceneEditorState>().tool, EditorTool::Inspect);
    assert_eq!(app.world().resource::<crate::viewport::modes::ViewportModeState>().mode, crate::viewport::modes::ViewportMode::Move);
    press(&mut app, &[KeyCode::KeyW]);
    assert!(!app.world().resource::<SceneEditorState>().placement_transform_enabled);
    assert_eq!(app.world().resource::<crate::viewport::modes::ViewportModeState>().mode, crate::viewport::modes::ViewportMode::Select);
    app.world_mut().resource_mut::<EditorSettings>().editor_keybindings.assign(EditorShortcutAction::MoveSelected, Some(EditorKeyBinding { control: true, ..EditorKeyBinding::plain(KeyCode::KeyM) })).unwrap();
    press(&mut app, &[KeyCode::KeyW]);
    assert!(!app.world().resource::<SceneEditorState>().placement_transform_enabled);
    press(&mut app, &[KeyCode::ControlRight, KeyCode::KeyM]);
    assert!(app.world().resource::<SceneEditorState>().placement_transform_enabled);
    assert!(!app.world().resource::<SceneEditorState>().dirty);
}

#[test]
fn move_tool_shortcut_respects_text_focus_mouse_drag_and_workspace() {
    for case in 0..5 {
        let mut app = app();
        match case {
            0 => app.world_mut().resource_mut::<ActiveNumericInput>().entity = Some(Entity::PLACEHOLDER),
            1 => app.world_mut().resource_mut::<super::super::text_input::EditorTextInputGuard>().block_shortcuts = true,
            2 => app.world_mut().resource_mut::<VegetationPresetEditorState>().active_tab = PresetWorkspaceTab::Document,
            3 => app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left),
            _ => app.world_mut().resource_mut::<SceneEditorNumericScrubMouseGuard>().advance(true, false, false),
        }
        press(&mut app, &[KeyCode::KeyW]);
        assert!(!app.world().resource::<SceneEditorState>().placement_transform_enabled, "隔离场景 {case}");
    }
}

#[test]
fn volume_radius_shortcuts_share_the_small_brush_range() {
    let mut app = app();
    for tool in [
        EditorTool::VolumeDig,
        EditorTool::VolumeFill,
        EditorTool::VolumeSmooth,
        EditorTool::VolumeFlatten,
    ] {
        {
            let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
            editor.tool = tool;
            editor.brush_radius = 0.75;
        }
        press(&mut app, &[KeyCode::BracketLeft]);
        assert_eq!(app.world().resource::<SceneEditorState>().brush_radius, 0.6);
        press(&mut app, &[KeyCode::BracketLeft]);
        assert_eq!(app.world().resource::<SceneEditorState>().brush_radius, 0.5);
        for _ in 0..20 {
            press(&mut app, &[KeyCode::BracketRight]);
        }
        assert_eq!(app.world().resource::<SceneEditorState>().brush_radius, 8.0);
    }
}

#[test]
fn custom_focus_binding_replaces_default_and_home_requires_no_extra_modifiers() {
    let mut app = app();
    select_portal(&mut app); // 改键测试必须真的有已确认对象，不能从空选择制造聚焦请求。
    app.world_mut()
        .resource_mut::<EditorSettings>()
        .editor_keybindings
        .assign(
            EditorShortcutAction::FocusSelected,
            Some(EditorKeyBinding {
                control: true,
                shift: true,
                ..EditorKeyBinding::plain(KeyCode::KeyG)
            }),
        )
        .unwrap();
    press(&mut app, &[KeyCode::KeyF]);
    assert!(
        app.world()
            .resource::<SceneEditorState>()
            .viewport_framing_requested
            .is_none()
    );
    press(
        &mut app,
        &[KeyCode::ControlLeft, KeyCode::ShiftRight, KeyCode::KeyG],
    );
    assert_eq!(
        app.world()
            .resource::<SceneEditorState>()
            .viewport_framing_requested,
        Some(EditorViewportFraming::Selected)
    );
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .viewport_framing_requested = None;
    press(&mut app, &[KeyCode::ControlLeft, KeyCode::Home]);
    assert!(
        app.world()
            .resource::<SceneEditorState>()
            .viewport_framing_requested
            .is_none()
    );
    press(&mut app, &[KeyCode::Home]);
    assert_eq!(
        app.world()
            .resource::<SceneEditorState>()
            .viewport_framing_requested,
        Some(EditorViewportFraming::Scene)
    );
}

#[test]
fn text_numeric_ime_other_tabs_loading_and_mouse_drags_block_navigation() {
    for case in 0..7 {
        let mut app = app();
        select_portal(&mut app); // 先满足业务资格，才能证明是输入所有权而非空选择阻止了导航。
        match case {
            0 => {
                let field = app
                    .world_mut()
                    .spawn(super::super::text_input::EditorTextInput)
                    .id();
                app.world_mut()
                    .resource_mut::<InputFocus>()
                    .set(field, bevy::input_focus::FocusCause::Navigated);
            }
            1 => {
                app.world_mut().resource_mut::<ActiveNumericInput>().entity =
                    Some(Entity::PLACEHOLDER)
            }
            2 => {
                app.world_mut()
                    .resource_mut::<super::super::text_input::EditorTextInputGuard>()
                    .block_shortcuts = true
            }
            3 => {
                app.world_mut()
                    .resource_mut::<VegetationPresetEditorState>()
                    .active_tab = PresetWorkspaceTab::Document
            }
            4 => {
                app.world_mut()
                    .resource_mut::<SceneEditorState>()
                    .scene_transition_in_progress = true
            }
            5 => app
                .world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .press(MouseButton::Right),
            _ => app
                .world_mut()
                .resource_mut::<SceneEditorNumericScrubMouseGuard>()
                .advance(true, false, false),
        }
        press(&mut app, &[KeyCode::KeyF, KeyCode::Home]);
        assert!(
            app.world()
                .resource::<SceneEditorState>()
                .viewport_framing_requested
                .is_none(),
            "隔离场景{case}"
        );
        assert!(!app.world().resource::<SceneEditorState>().dirty);
    }
}

#[test]
fn existing_save_history_and_brush_keys_use_configured_bindings_without_repeat() {
    let mut app = app();
    let command = crate::authoring::EditorCommand::TerrainChanged(vec![crate::authoring::HeightChange {
        index: 0, before: 0.0, after: 1.0,
    }]);
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        session.undo.push(command.clone());
        session.redo.push(command);
    } // 分派回归只登记请求；历史必须包含真实命令，而不是允许空栈快捷键。
    app.world_mut()
        .resource_mut::<EditorSettings>()
        .editor_keybindings
        .assign(
            EditorShortcutAction::Save,
            Some(EditorKeyBinding::plain(KeyCode::KeyK)),
        )
        .unwrap();
    press(&mut app, &[KeyCode::ControlLeft, KeyCode::KeyS]);
    assert!(!app.world().resource::<SceneEditorState>().save_requested);
    press(&mut app, &[KeyCode::KeyK]);
    assert!(app.world().resource::<SceneEditorState>().save_requested);
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .save_requested = false;
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .clear();
    app.update();
    assert!(!app.world().resource::<SceneEditorState>().save_requested);
    press(&mut app, &[KeyCode::ControlRight, KeyCode::KeyZ]);
    assert!(app.world().resource::<SceneEditorState>().undo_requested);
    press(&mut app, &[KeyCode::ControlLeft, KeyCode::KeyY]);
    assert!(app.world().resource::<SceneEditorState>().redo_requested);
    let before = app.world().resource::<SceneEditorState>().brush_radius;
    press(&mut app, &[KeyCode::BracketRight]);
    assert!(app.world().resource::<SceneEditorState>().brush_radius > before);
}

#[test]
fn right_plugin_tool_owns_history_even_while_central_scene_stays_visible() {
    use crate::ui::sidebars::{SceneEditorSidebarState, EditorLeftSidebarTab, EditorRightSidebarTab};
    let mut app = app();
    app.insert_resource(crate::game_authoring::DomainToolLabels([("fixture.tool", "插件工具".into())].into()))
        .init_resource::<SceneEditorSidebarState>();
    {
        let mut sidebar=app.world_mut().resource_mut::<SceneEditorSidebarState>();
        sidebar.left=EditorLeftSidebarTab::Domain("fixture.tool");
        sidebar.right=EditorRightSidebarTab::Tools;
    }
    press(&mut app, &[KeyCode::ControlLeft, KeyCode::KeyS]);
    assert!(!app.world().resource::<SceneEditorState>().save_requested);
    assert_eq!(app.world().resource::<VegetationPresetEditorState>().active_tab, PresetWorkspaceTab::Scene);
    app.world_mut().resource_mut::<SceneEditorSidebarState>().right=EditorRightSidebarTab::Scene;
    press(&mut app, &[KeyCode::ControlLeft, KeyCode::KeyS]);
    assert!(app.world().resource::<SceneEditorState>().save_requested, "返回属性后场景恢复原快捷键");
}
