//! 用普通移动工具相同的输入所有权与预览根，核对射程、锥角、取消和约束。

use super::*;
use crate::viewport::placement_move::{LightHandle, validation_drag_points};
use crate::viewport::modes::ViewportMode;
use bevy::ecs::system::RunSystemOnce;

fn fixture(spot: bool, inner: f32) -> (App, Entity, Entity) {
    let mut lamp = record();
    lamp.asset_path.clear();
    lamp.water_surfaces.clear();
    lamp.asset_target = crate::authoring::local_lights::target(([0.2, 0.6, 1.0], 670.0, Some(7.0),
        spot.then_some([inner.to_radians(), 35f32.to_radians()]))).unwrap();
    let (mut app, root, node) = app_with_record(lamp.clone());
    app.world_mut().run_system_once(move |mut commands: Commands| {
        assert!(feathrise_scene::spawn_builtin_placement_visual(&mut commands, root, lamp.asset_target.clone()));
    }).unwrap();
    app.world_mut().resource_mut::<ViewportModeState>().mode = ViewportMode::Select;
    app.world_mut().resource_mut::<SceneEditorState>().placement_transform_enabled = false;
    let mut view = app.world_mut().resource_mut::<SceneEditorViewportState>();
    view.yaw = 0.7; view.pitch = 0.32; view.extent = 30.0;
    (app, root, node)
}

fn pull(app: &mut App, node: Entity, handle: LightHandle, goal: f32) -> Vec2 {
    let (start, end) = validation_drag_points(app.world_mut(), handle, goal).expect("可见光界应有独立拾取位置");
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
    step(app, node, start);
    step(app, node, start.lerp(end, 0.5));
    step(app, node, end);
    end
}

fn component_value(app: &App, root: Entity, handle: LightHandle) -> f32 {
    if let Some(light) = app.world().get::<PointLight>(root) { return light.range; }
    let light = app.world().get::<SpotLight>(root).unwrap();
    match handle { LightHandle::Range => light.range, LightHandle::Outer => light.outer_angle.to_degrees(), LightHandle::Inner => light.inner_angle.to_degrees() }
}

#[test]
fn local_light_boundaries_preview_live_and_release_only_one_history_in_selection_mode() {
    if !crate::authoring::local_lights::AVAILABLE { return; }
    for (spot, handle, goal) in [(false, LightHandle::Range, 9.0), (true, LightHandle::Range, 9.0),
        (true, LightHandle::Inner, 19.0), (true, LightHandle::Outer, 48.0)] {
        let (mut app, root, node) = fixture(spot, 10.0);
        let before = draft_record(&app).clone();
        let end = pull(&mut app, node, handle, goal);
        let session = app.world().resource::<EditorAuthoringSession>();
        assert!(session.undo.is_empty() && draft_record(&app) == &before);
        let preview = app.world().resource::<PlacementMoveState>().light_preview(&session.draft.as_ref().unwrap().scene_id, &before.id).unwrap();
        assert!((handle.value(preview).unwrap() - goal).abs() < 0.08, "{handle:?}: {:?}", handle.value(preview));
        assert!((component_value(&app, root, handle) - goal).abs() < 0.08);
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
        step(&mut app, node, end);
        assert!((component_value(&app, root, handle) - goal).abs() < 0.08, "松手不能让渲染组件回到拖动前");
        let after = draft_record(&app);
        assert!((handle.value(after).unwrap() - goal).abs() < 0.08);
        let mut same_pose = before.clone(); same_pose.asset_target = after.asset_target.clone();
        assert_eq!(after, &same_pose);
        assert!(matches!(app.world().resource::<EditorAuthoringSession>().undo.as_slice(),
            [EditorCommand::PlacementChanged { before: a, after: b, .. }] if a == &before && b == after));
    }
}

#[test]
fn local_light_boundaries_cancel_on_escape_or_selection_change_and_enforce_cone_limits() {
    if !crate::authoring::local_lights::AVAILABLE { return; }
    for escape in [true, false] {
        let (mut app, root, node) = fixture(true, 10.0);
        let before = draft_record(&app).clone();
        let end = pull(&mut app, node, LightHandle::Inner, 55.0);
        let preview = app.world().get::<SpotLight>(root).unwrap();
        assert!(preview.inner_angle < preview.outer_angle && preview.inner_angle.to_degrees() > 34.8);
        if escape { app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Escape); }
        else { app.world_mut().resource_mut::<EditorGltfBrowserState>().selected_placement_id = None; }
        step(&mut app, node, end);
        assert!(app.world().resource::<PlacementMoveState>().drag.is_none());
        assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
        assert_eq!(draft_record(&app), &before);
        assert!((app.world().get::<SpotLight>(root).unwrap().inner_angle.to_degrees() - 10.0).abs() < 0.001);
    }
}

#[test]
fn local_light_default_zero_inner_cone_opens_by_dragging_the_axis() {
    if !crate::authoring::local_lights::AVAILABLE { return; }
    let (mut app, root, node) = fixture(true, 0.0);
    pull(&mut app, node, LightHandle::Inner, 12.0);
    let angle = app.world().get::<SpotLight>(root).unwrap().inner_angle.to_degrees();
    assert!((angle - 12.0).abs() < 1.0, "零度内锥拖离轴线后得到 {angle}°");
}
