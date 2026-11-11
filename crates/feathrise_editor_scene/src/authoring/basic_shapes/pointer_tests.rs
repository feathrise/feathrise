//! 统一面板请求与真实释放点击的集成回归，覆盖跨中央标签、坐标中心及取消身份。

use super::*;
use crate::{
    authoring::EditorAuthoringSession,
    resources::SceneEditorState,
    ui::{sidebars::{EditorLeftSidebarTab, EditorRightSidebarTab, SceneEditorSidebarState},
        vegetation_preset_editor::{PresetWorkspaceTab, VegetationPresetEditorState}},
};
use bevy::{prelude::*, input::{ButtonState, keyboard::{Key, KeyboardInput}, mouse::MouseButtonInput}};
use feathrise_scene_content::ScenePlacementAssetTarget;

fn page() -> (App, Entity) {
    let (mut app, window) = crate::ui::terrain_objects::page_tests::creation_page();
    {
        let mut state = app.world_mut().resource_mut::<crate::ui::terrain_objects::TerrainObjectsEditorState>();
        state.creation_source = crate::ui::terrain_objects::CreationSource::Cuboid;
        state.placement_mode = crate::ui::terrain_objects::CreationPlacementMode::Viewport;
    }
    app.init_resource::<BasicShapeState>()
        .init_resource::<VegetationPresetEditorState>()
        .init_resource::<crate::ui::courtyard::CourtyardUiState>()
        .init_asset::<StandardMaterial>()
        .add_systems(Update, systems::control_menu.after(crate::ui::terrain_objects::CreationRequestDispatch).before(crate::ui::numeric_scrub_mouse_guard::update_scene_editor_numeric_scrub_mouse_guard).in_set(crate::SceneEditorSystemSet::Input))
        .add_systems(Update, crate::ui::numeric_scrub_mouse_guard::update_scene_editor_numeric_scrub_mouse_guard.in_set(crate::SceneEditorSystemSet::Input))
        .add_systems(Update, crate::viewport::modes::sync_mode.after(crate::SceneEditorSystemSet::Input).before(crate::SceneEditorSystemSet::Tasks));
    app.update(); // 先走正式编辑器首次打开的安全选择门，后续Start才属于用户显式输入。
    (app, window)
}

fn request(app: &mut App, position: Option<Vec3>) {
    let identity = { let session = app.world().resource::<EditorAuthoringSession>(); (session.draft.as_ref().unwrap().scene_id.clone(), session.revision) };
    let mut state = app.world_mut().resource_mut::<BasicShapeState>();
    state.creation_identity = Some(identity);
    state.creation_request = Some(CuboidCreationRequest { target: ScenePlacementAssetTarget::Cuboid {
        size_mm: [3000, 4000, 5000], color_srgb: [100, 150, 200], roughness_milli: 250, collision: false,
    }, position });
}

#[test]
fn real_start_button_only_arms_point_placement_after_release_and_the_request_is_consumed() {
    let (mut app, window) = page();
    app.world_mut().resource_mut::<BasicShapeState>().request_menu = true;
    for _ in 0..5 { app.update(); }
    let button = {
        let world = app.world_mut();
        world.query::<(&Text, &ChildOf)>().iter(world)
            .find_map(|(text, parent)| (text.as_str() == "开始点放").then_some(parent.parent())).expect("统一面板正式开始按钮")
    };
    let point = app.world().get::<UiGlobalTransform>(button).unwrap().translation;
    assert!(app.world().get::<ComputedNode>(button).unwrap().size().min_element() > 0.0);
    app.world_mut().get_mut::<Window>(window).unwrap().set_cursor_position(Some(point));
    app.world_mut().write_message(bevy::window::WindowEvent::CursorMoved(bevy::window::CursorMoved { window, position: point, delta: None }));
    app.update();
    for state in [ButtonState::Pressed, ButtonState::Released] {
        let event = MouseButtonInput { window, state, button: MouseButton::Left };
        app.world_mut().write_message(event);
        app.world_mut().write_message(bevy::window::WindowEvent::MouseButtonInput(event));
        app.update();
        if state == ButtonState::Pressed { assert!(!is_cuboid_selection(app.world().resource::<SceneEditorState>())); }
    }
    for _ in 0..3 { app.update(); }
    assert!(is_cuboid_selection(app.world().resource::<SceneEditorState>()));
    let session = app.world().resource::<EditorAuthoringSession>();
    assert!(app.world().resource::<BasicShapeState>().placement_target(session).is_some());
    assert!(session.undo.is_empty(), "开始点放不提前创建作者对象");
    let cancel = {
        let world = app.world_mut();
        world.query::<(&Text, &ChildOf)>().iter(world)
            .find_map(|(text, parent)| (text.as_str() == "取消点放").then_some(parent.parent())).unwrap()
    };
    assert!(app.world().get::<bevy::ui::InteractionDisabled>(cancel).is_none());
    let point = app.world().get::<UiGlobalTransform>(cancel).unwrap().translation;
    app.world_mut().get_mut::<Window>(window).unwrap().set_cursor_position(Some(point));
    app.world_mut().write_message(bevy::window::WindowEvent::CursorMoved(bevy::window::CursorMoved { window, position: point, delta: None }));
    app.update();
    for state in [ButtonState::Pressed, ButtonState::Released] {
        let event = MouseButtonInput { window, state, button: MouseButton::Left };
        app.world_mut().write_message(event);
        app.world_mut().write_message(bevy::window::WindowEvent::MouseButtonInput(event));
        app.update();
        if state == ButtonState::Pressed { assert!(is_cuboid_selection(app.world().resource::<SceneEditorState>())); }
    }
    assert!(!is_cuboid_selection(app.world().resource::<SceneEditorState>()));
    assert!(app.world().resource::<SceneEditorState>().selected_asset.is_none());
    let state = app.world().resource::<BasicShapeState>();
    assert!(state.placement_target.is_none() && state.placement_identity.is_none() && state.work_plane.is_none());
    assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
}

#[test]
fn shared_mesh_panel_opens_on_other_tabs_and_coordinate_creation_preserves_the_center() {
    for tab in [PresetWorkspaceTab::Document, PresetWorkspaceTab::AssetPreview] {
        let (mut app, _) = page();
        app.world_mut().resource_mut::<VegetationPresetEditorState>().active_tab = tab;
        app.world_mut().resource_mut::<BasicShapeState>().request_menu = true;
        app.update();
        assert_eq!(app.world().resource::<VegetationPresetEditorState>().active_tab, tab);
        let sidebar = app.world().resource::<SceneEditorSidebarState>();
        assert_eq!(sidebar.left, EditorLeftSidebarTab::Meshes);
        assert_eq!(sidebar.right, EditorRightSidebarTab::Tools);
        request(&mut app, Some(Vec3::new(7.0, 8.0, 9.0)));
        app.update();
        let session = app.world().resource::<EditorAuthoringSession>();
        let record = &session.draft.as_ref().unwrap().placements.placements[0];
        assert_eq!(record.translation, [7.0, 8.0, 9.0]);
        assert!(matches!(record.asset_target, ScenePlacementAssetTarget::Cuboid { size_mm: [3000, 4000, 5000], collision: false, roughness_milli: 250, .. }));
        assert_eq!(session.undo.len(), 1);
        assert_eq!(app.world().resource::<VegetationPresetEditorState>().active_tab, tab);
    }
}

#[test]
fn point_placement_switches_to_scene_and_esc_cancels_without_history() {
    let (mut app, window) = page();
    app.world_mut().resource_mut::<VegetationPresetEditorState>().active_tab = PresetWorkspaceTab::Document;
    request(&mut app, None);
    app.update();
    assert_eq!(app.world().resource::<VegetationPresetEditorState>().active_tab, PresetWorkspaceTab::Scene);
    assert!(is_cuboid_selection(app.world().resource::<SceneEditorState>()));
    app.world_mut().write_message(KeyboardInput { key_code: KeyCode::Escape, logical_key: Key::Escape, state: ButtonState::Pressed, text: None, repeat: false, window });
    app.update();
    assert!(!is_cuboid_selection(app.world().resource::<SceneEditorState>()));
    assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
    assert!(!app.world().resource::<SceneEditorState>().close_requested);
}

#[test]
fn closing_or_changing_the_creation_scope_cancels_an_armed_cube_without_author_data() {
    for case in 0..5 {
        let (mut app, _) = page();
        request(&mut app, None);
        app.update();
        assert!(is_cuboid_selection(app.world().resource::<SceneEditorState>()));
        match case {
            0 => app.world_mut().resource_mut::<SceneEditorState>().close_requested = true,
            1 => app.world_mut().resource_mut::<EditorAuthoringSession>().revision += 1,
            2 => app.world_mut().resource_mut::<crate::ui::terrain_objects::TerrainObjectsEditorState>().creation_source = crate::ui::terrain_objects::CreationSource::Heightfield,
            3 => app.world_mut().resource_mut::<crate::ui::terrain_objects::TerrainObjectsEditorState>().placement_mode = crate::ui::terrain_objects::CreationPlacementMode::Coordinates,
            _ => app.world_mut().resource_mut::<VegetationPresetEditorState>().active_tab = PresetWorkspaceTab::Document,
        }
        app.update();
        assert!(!is_cuboid_selection(app.world().resource::<SceneEditorState>()), "取消场景{case}");
        assert!(app.world().resource::<SceneEditorState>().selected_asset.is_none());
        let state = app.world().resource::<BasicShapeState>();
        assert!(state.placement_target.is_none() && state.placement_identity.is_none());
        assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
    }
}

#[test]
fn stale_creation_request_cannot_be_adopted_by_a_new_author_revision() {
    let (mut app, _) = page();
    request(&mut app, Some(Vec3::ONE));
    app.world_mut().resource_mut::<EditorAuthoringSession>().revision += 1;
    app.update();
    assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
    assert!(app.world().resource::<BasicShapeState>().creation_request.is_none());
}
