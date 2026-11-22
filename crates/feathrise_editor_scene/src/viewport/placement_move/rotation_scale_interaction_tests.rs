//! 真实输入与模型根回归：旋转/缩放只预览，松手一次提交；建筑仍使用真实异步校验。

use super::super::{constants::ROTATION_RADIUS_FRACTION, rotation::yaw_direction};
use super::*;
use crate::viewport::modes::ViewportMode;
use crate::authoring::placement_pose::placement_rotation;
use feathrise_editor_animation_model::gizmo::{MoveConstraint, rotation_rings};
use super::super::{scale::ScaleGizmo, types::MovePlane};

fn prepare(app: &mut App, root: Entity, mode: ViewportMode, view: EditorView, orthographic: bool) {
    app.world_mut().resource_mut::<ViewportModeState>().mode = mode;
    plane_tests::set_camera(app, view, orthographic);
    let record = draft_record(app).clone();
    app.world_mut()
        .resource_mut::<SceneEditorViewportState>()
        .center = Vec3::from_array(record.translation);
    *app.world_mut().get_mut::<Transform>(root).unwrap() = transform(&record);
}

fn transform(record: &ScenePlacementRecord) -> Transform {
    Transform {
        translation: Vec3::from_array(record.translation),
        rotation: placement_rotation(record),
        scale: Vec3::from_array(record.scale),
    }
}

fn cursor(app: &App, mode: ViewportMode, delta: f32) -> Vec2 {
    let camera = camera(app);
    let state = app.world().resource::<PlacementMoveState>();
    let before = state
        .drag
        .as_ref()
        .map_or_else(|| draft_record(app), |drag| &drag.before);
    let origin = Vec3::from_array(before.translation);
    if mode == ViewportMode::Rotate {
        yaw_cursor(camera, origin, super::super::input::yaw_only(before), delta).0
    } else {
        camera.project(origin).unwrap() + Vec2::X * delta / camera.size / camera.inverse_scale
    }
}

pub(super) fn yaw_cursor(camera: MoveCamera, origin: Vec3, yaw_only: bool, delta: f32) -> (Vec2, MoveHandle) {
    if yaw_only {
        let radius = camera.handle_length(origin).unwrap() * ROTATION_RADIUS_FRACTION;
        return (camera.project(origin + yaw_direction(delta) * radius).unwrap(), MoveHandle::RotationY);
    }
    let handle = MoveHandle::AnimationRotation(MoveConstraint::Y);
    let start = rotation_rings(*camera, origin).into_iter().flatten()
        .find(|ring| ring.constraint == MoveConstraint::Y).unwrap().world.into_iter().find(|point| {
            let cursor = camera.project(*point).unwrap();
            super::super::animation::pick_rotation(camera, origin, cursor) == Some(handle)
        }).unwrap();
    let point = origin + Quat::from_rotation_y(delta.to_radians()) * (start - origin);
    (camera.project(point).unwrap(), handle)
}

fn start(app: &mut App, node: Entity, mode: ViewportMode) {
    let cursor = cursor(app, mode, 0.0);
    let expected = if mode == ViewportMode::Rotate {
        let record = draft_record(app);
        yaw_cursor(camera(app), record.translation.into(), super::super::input::yaw_only(record), 0.0).1
    } else {
        MoveHandle::UniformScale
    };
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    step(app, node, cursor);
    let state = app.world().resource::<PlacementMoveState>();
    assert_eq!(state.drag.as_ref().map(|drag| drag.handle), Some(expected));
    assert!(state.owns_mouse());
    assert!(
        app.world()
            .resource::<SceneEditorNumericScrubMouseGuard>()
            .blocked()
    );
}

fn release(app: &mut App, node: Entity, cursor: Vec2) {
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    step(app, node, cursor);
}

fn assert_root(app: &App, root: Entity, expected: &ScenePlacementRecord) {
    let root = app.world().get::<Transform>(root).unwrap();
    let expected = transform(expected);
    assert!(root.translation.distance(expected.translation) < 0.002);
    assert!(root.rotation.angle_between(expected.rotation).abs() < 0.002);
    assert!(root.scale.distance(expected.scale) < 0.002);
}

#[test]
fn captured_move_rotation_and_scale_only_show_guides_while_held() {
    use crate::viewport::tool_preview::{AuthorToolFrame, foreground::Foreground};
    for mode in [ViewportMode::Move, ViewportMode::Rotate, ViewportMode::Scale] {
        let (mut app, root, node) = app();
        app.world_mut().resource_mut::<EditorAuthoringSession>().draft.as_mut().unwrap()
            .placements.placements[0].yaw_degrees = 0.;
        prepare(&mut app, root, mode, EditorView::Perspective, false);
        app.init_resource::<AuthorToolFrame>().init_resource::<Foreground>()
            .add_systems(First, |mut frame: ResMut<AuthorToolFrame>, mut geometry: ResMut<Foreground>| {
                *frame = default();
                *geometry = default();
            })
            .add_systems(Update, super::super::render::draw_placement_move.after(handle_placement_move));
        let camera = camera(&app);
        let record = draft_record(&app);
        let origin = Vec3::from_array(record.translation);
        let (pointer, handle) = match mode {
            ViewportMode::Move => (
                camera.project(origin + Vec3::X * camera.handle_length(origin).unwrap() * 0.65).unwrap(),
                MoveHandle::Axis(MoveAxis::X),
            ),
            ViewportMode::Rotate => yaw_cursor(camera, origin, super::super::input::yaw_only(record), 0.),
            ViewportMode::Scale => {
                let scale = ScaleGizmo::new(camera, origin, Mat3::IDENTITY, false).unwrap();
                (camera.project(scale.axes[0].as_ref().unwrap().tip).unwrap(), MoveHandle::AxisScale(MoveAxis::X))
            }
            _ => unreachable!(),
        };
        step(&mut app, node, pointer);
        assert_eq!(app.world().resource::<PlacementMoveState>().hover, Some(handle));
        assert!(!app.world().resource::<Foreground>().contains_viewport_guide());
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        step(&mut app, node, pointer);
        assert_eq!(app.world().resource::<PlacementMoveState>().drag.as_ref().map(|drag| drag.handle), Some(handle));
        assert!(app.world().resource::<Foreground>().contains_viewport_guide(), "{mode:?} 捕获期间需要贯穿轴线");
        release(&mut app, node, pointer);
        assert!(!app.world().resource::<Foreground>().contains_viewport_guide());
    }
}

#[test]
fn scale_axis_plane_and_outer_ring_update_only_their_scale_components() {
    for handle in [MoveHandle::AxisScale(MoveAxis::X), MoveHandle::PlaneScale(MovePlane::Xy), MoveHandle::UniformScale] {
        let (mut app, root, node) = app();
        app.world_mut().resource_mut::<EditorAuthoringSession>().draft.as_mut().unwrap().placements.placements[0].yaw_degrees = 0.0;
        prepare(&mut app, root, ViewportMode::Scale, EditorView::Front, true);
        let camera = camera(&app);
        let before = draft_record(&app).clone();
        let origin = Vec3::from_array(before.translation);
        let geometry = ScaleGizmo::new(camera, origin, Mat3::IDENTITY, false).unwrap();
        let point = match handle {
            MoveHandle::AxisScale(_) => geometry.axes[MoveAxis::X.index()].as_ref().unwrap().tip,
            MoveHandle::PlaneScale(plane) => {
                let corners = geometry.planes[MovePlane::ALL.iter().position(|shown| *shown == plane).unwrap()].unwrap();
                corners[0].lerp(corners[2], 0.5)
            }
            _ => geometry.outer.unwrap()[0],
        };
        let start = camera.project(point).unwrap();
        let center = camera.project(origin).unwrap();
        let logical = camera.size * camera.inverse_scale;
        let direction = ((start - center) * logical).normalize();
        let target = start + direction * 30.0 / logical;
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        step(&mut app, node, start);
        assert_eq!(app.world().resource::<PlacementMoveState>().drag.as_ref().map(|drag| drag.handle), Some(handle));
        step(&mut app, node, target);
        assert_eq!(draft_record(&app), &before);
        let mut expected = before.clone();
        for axis in MoveAxis::ALL {
            if handle.scales_axis(axis) { expected.scale[axis.index()] *= 2.0_f32.powf(0.25); }
        }
        assert_root(&app, root, &expected);
        release(&mut app, node, target);
        assert!(Vec3::from_array(draft_record(&app).scale).abs_diff_eq(Vec3::from_array(expected.scale), 0.002));
        assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 1);
    }
}

#[test]
fn combined_shape_leaves_separate_movement_rotation_and_scale_hit_regions() {
    let (mut app, root, node) = app();
    prepare(&mut app, root, ViewportMode::Transform, EditorView::Top, true);
    let camera = camera(&app);
    let record = draft_record(&app);
    let origin = Vec3::from_array(record.translation);
    let basis = Mat3::from_quat(placement_rotation(record));
    let geometry = ScaleGizmo::new(camera, origin, basis, true).unwrap();
    assert!(geometry.outer.is_none());
    let length = camera.handle_length(origin).unwrap();
    let (rotate, handle) = if super::super::input::yaw_only(record) {
        (camera.project(origin + Vec3::NEG_X * length * ROTATION_RADIUS_FRACTION).unwrap(), MoveHandle::RotationY)
    } else {
        (camera.project(origin + Vec3::NEG_X * length * 0.9 * 1.2).unwrap(), MoveHandle::AnimationRotation(MoveConstraint::View))
    };
    for (cursor, expected) in [
        (camera.project(origin + Vec3::X * length * 0.5).unwrap(), MoveHandle::Axis(MoveAxis::X)),
        (camera.project(origin).unwrap(), MoveHandle::UniformScale),
        (camera.project(geometry.axes[0].as_ref().unwrap().tip).unwrap(), MoveHandle::AxisScale(MoveAxis::X)),
        (rotate, handle),
    ] {
        step(&mut app, node, cursor);
        assert_eq!(app.world().resource::<PlacementMoveState>().hover, Some(expected));
    }
}

#[test]
fn custom_rotation_and_scale_snap_steps_are_frozen_for_each_drag() {
    for mode in [ViewportMode::Rotate, ViewportMode::Scale] {
        let (mut app, root, node) = app();
        prepare(&mut app, root, mode, EditorView::Top, false);
        app.insert_resource(feathrise_editor_core::EditorSettings { editor_snapping: feathrise_editor_core::EditorSnapSettings {
            rotation_degrees: 22.5, scale_factor: 0.25, ..default() }, ..default() });
        let before = draft_record(&app).clone();
        start(&mut app, node, mode);
        app.world_mut().resource_mut::<feathrise_editor_core::EditorSettings>().editor_snapping = default();
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::ControlLeft);
        let target = cursor(&app, mode, if mode == ViewportMode::Rotate { 24.0 } else { 35.0 });
        step(&mut app, node, target);
        let mut expected = before.clone();
        if mode == ViewportMode::Rotate { expected.yaw_degrees = 67.5; }
        else { expected.scale = before.scale.map(|value| value * 1.25); }
        assert_root(&app, root, &expected);
        assert_eq!(draft_record(&app), &before);
        release(&mut app, node, target);
        assert_root(&app, root, &expected);
    }
}

#[test]
fn rotation_scale_live_preview_preserves_other_fields_and_commits_once() {
    for mode in [ViewportMode::Rotate, ViewportMode::Scale] {
        for view in [
            EditorView::Top,
            EditorView::Perspective,
            EditorView::Front,
            EditorView::Side,
        ] {
            if mode == ViewportMode::Rotate && matches!(view, EditorView::Front | EditorView::Side)
            {
                continue;
            }
            for orthographic in [false, true] {
                for snap in [false, true] {
                    let (mut app, root, node) = app();
                    prepare(&mut app, root, mode, view, orthographic);
                    let before = draft_record(&app).clone();
                    start(&mut app, node, mode);
                    if snap {
                        app.world_mut()
                            .resource_mut::<ButtonInput<KeyCode>>()
                            .press(KeyCode::ControlLeft);
                    }
                    let delta = if mode == ViewportMode::Rotate {
                        if snap { 19.0 } else { 23.0 }
                    } else if snap {
                        1.26_f32.log2() * 120.0
                    } else {
                        120.0
                    };
                    let cursor = cursor(&app, mode, delta);
                    for _ in 0..3 {
                        step(&mut app, node, cursor);
                    }
                    let mut expected = before.clone();
                    if mode == ViewportMode::Rotate {
                        expected.yaw_degrees = 60.0;
                    } else {
                        expected.scale = before
                            .scale
                            .map(|value| value * if snap { 1.3 } else { 2.0 });
                    }
                    assert_root(&app, root, &expected);
                    assert_eq!(draft_record(&app), &before);
                    assert!(
                        app.world()
                            .resource::<EditorAuthoringSession>()
                            .undo
                            .is_empty()
                    );
                    assert!(!app.world().resource::<SceneEditorState>().dirty);
                    release(&mut app, node, cursor);
                    let after = draft_record(&app);
                    assert_root(&app, root, after);
                    assert!((after.yaw_degrees - expected.yaw_degrees).abs() < 0.05);
                    assert!(
                        Vec3::from_array(after.scale).distance(Vec3::from_array(expected.scale))
                            < 0.002
                    );
                    assert_eq!(after.translation, before.translation);
                    assert_eq!(after.height_mode, before.height_mode);
                    assert_eq!(after.terrain_offset, before.terrain_offset);
                    assert_eq!(after.water_surfaces, before.water_surfaces);
                    let session = app.world().resource::<EditorAuthoringSession>();
                    assert_eq!(session.revision, 8);
                    assert_eq!(session.undo.len(), 1);
                    assert!(
                        matches!(&session.undo[0], EditorCommand::PlacementChanged { before: old, after: new, .. } if old == &before && new == after)
                    );
                    assert!(app.world().resource::<PlacementMoveState>().owns_mouse());
                    step(&mut app, node, cursor);
                    assert!(!app.world().resource::<PlacementMoveState>().owns_mouse());
                }
            }
        }
    }
}

#[test]
fn rotation_scale_cancel_and_source_changes_restore_current_full_transform() {
    for mode in [ViewportMode::Rotate, ViewportMode::Scale] {
        for reason in 0..6 {
            let (mut app, root, node) = app();
            prepare(&mut app, root, mode, EditorView::Top, false);
            start(&mut app, node, mode);
            let cursor = cursor(&app, mode, 70.0);
            step(&mut app, node, cursor);
            match reason {
                0 => app
                    .world_mut()
                    .resource_mut::<ButtonInput<KeyCode>>()
                    .press(KeyCode::Escape),
                1 => app
                    .world_mut()
                    .resource_mut::<ButtonInput<MouseButton>>()
                    .press(MouseButton::Right),
                2 => app.world_mut().resource_mut::<ViewportModeState>().mode = ViewportMode::Move,
                3 => {
                    app.world_mut()
                        .resource_mut::<SceneEditorViewportState>()
                        .render_size
                        .x += 1
                }
                4 => {
                    app.world_mut()
                        .resource_mut::<EditorAuthoringSession>()
                        .draft
                        .as_mut()
                        .unwrap()
                        .placements
                        .placements[0]
                        .yaw_degrees = 111.0
                }
                _ => {
                    app.world_mut()
                        .resource_mut::<EditorAuthoringSession>()
                        .draft
                        .as_mut()
                        .unwrap()
                        .placements
                        .placements[0]
                        .scale = [3.0, 4.0, 5.0]
                }
            }
            step(&mut app, node, cursor);
            assert!(app.world().resource::<PlacementMoveState>().drag.is_none());
            assert_root(&app, root, draft_record(&app));
            let session = app.world().resource::<EditorAuthoringSession>();
            assert_eq!(session.revision, 7);
            assert!(session.undo.is_empty());
            assert!(!app.world().resource::<SceneEditorState>().dirty);
            assert!(app.world().resource::<PlacementMoveState>().owns_mouse());
            app.world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .release(MouseButton::Right);
            release(&mut app, node, cursor);
            assert!(app.world().resource::<PlacementMoveState>().owns_mouse());
            step(&mut app, node, cursor);
            assert!(!app.world().resource::<PlacementMoveState>().owns_mouse());
        }
    }
}

#[test]
fn rotation_scale_click_or_return_to_start_preserves_redo_and_off_grid_values() {
    for mode in [ViewportMode::Rotate, ViewportMode::Scale] {
        for moved in [false, true] {
            let (mut app, root, node) = app();
            prepare(&mut app, root, mode, EditorView::Top, true);
            let before = draft_record(&app).clone();
            app.world_mut()
                .resource_mut::<EditorAuthoringSession>()
                .redo
                .push(EditorCommand::PlacementAdded(before.clone()));
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::ControlLeft);
            start(&mut app, node, mode);
            if moved {
                let cursor = cursor(&app, mode, 80.0);
                step(&mut app, node, cursor);
            }
            let cursor = cursor(&app, mode, 0.0);
            release(&mut app, node, cursor);
            assert_root(&app, root, &before);
            assert_eq!(draft_record(&app), &before);
            let session = app.world().resource::<EditorAuthoringSession>();
            assert!(session.undo.is_empty());
            assert_eq!(session.redo.len(), 1);
            assert_eq!(session.revision, 7);
            assert!(!app.world().resource::<SceneEditorState>().dirty);
        }
    }
}

#[test]
fn rotation_scale_do_not_resample_follow_terrain_height() {
    for mode in [ViewportMode::Rotate, ViewportMode::Scale] {
        let (mut app, root, node) = app();
        {
            let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
            let draft = session.draft.as_mut().unwrap();
            draft.placements.placements[0].height_mode = ScenePlacementHeightMode::FollowTerrain;
            draft.placements.placements[0].terrain_offset = 2.0;
            draft.heights = None; // 只有平移需要重新采样；旋转/缩放不能因未驻留的高度页改变原Y。
            draft.terrain_ground = None;
        }
        prepare(&mut app, root, mode, EditorView::Top, false);
        let before = draft_record(&app).clone();
        start(&mut app, node, mode);
        let cursor = cursor(&app, mode, 70.0);
        step(&mut app, node, cursor);
        assert!(
            app.world()
                .resource::<PlacementMoveState>()
                .drag
                .as_ref()
                .unwrap()
                .valid
        );
        release(&mut app, node, cursor);
        let after = draft_record(&app);
        assert_eq!(after.translation, before.translation);
        assert_eq!(after.terrain_offset, before.terrain_offset);
        assert_eq!(
            app.world().resource::<EditorAuthoringSession>().undo.len(),
            1
        );
    }
}

#[test]
fn rotation_scale_building_scale_does_not_arm() {
    let (mut app, root, node) = app();
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .draft
        .as_mut()
        .unwrap()
        .placements
        .placements[0]
        .building_preset_id = Some("test-building".into());
    prepare(&mut app, root, ViewportMode::Scale, EditorView::Top, false);
    let before = draft_record(&app).clone();
    let cursor = cursor(&app, ViewportMode::Scale, 0.0);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    step(&mut app, node, cursor);
    assert!(app.world().resource::<PlacementMoveState>().drag.is_none());
    assert!(!app.world().resource::<PlacementMoveState>().owns_mouse());
    release(&mut app, node, cursor);
    assert_eq!(draft_record(&app), &before);
    assert_root(&app, root, &before);
}

#[test]
fn rotation_scale_building_rotation_waits_for_real_validation_and_restores_on_failure() {
    use crate::authoring::buildings::move_test_support::{
        ValidationPollingEnabled, install_existing_edit_validator,
    };
    use feathrise_scene_content::BuildingPresetDescriptor;
    for (delta, expected_yaw, accepted, formal_root) in [
        (-100.0, 270.0_f32, true, false),
        (200.0, 180.0, true, true),
        (90.0, 90.0, false, false),
        (90.0, 90.0, false, true),
    ] {
        let (mut app, root, node) = app();
        {
            let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
            let record = &mut session.draft.as_mut().unwrap().placements.placements[0];
            record.building_preset_id = Some("test-building".into());
            record.yaw_degrees = 0.0;
            record.scale = [1.0; 3];
            if !accepted {
                record.translation[0] = 1.0;
            }
        }
        let before = draft_record(&app).clone();
        if formal_root {
            app.add_observer(crate::ui::placement_water::register_placement_instance_root);
            let map = {
                let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
                session.placement_previews.clear();
                session.draft.as_ref().unwrap().scene_id.clone()
            };
            app.world_mut()
                .entity_mut(root)
                // 基础夹具已带作者身份，正式根须重新添加组件触发Add观察者登记真实活动索引。
                .remove::<feathrise_scene::ScenePlacementInstance>()
                .insert(feathrise_scene::ScenePlacementInstance::new(&map, &before));
        }
        let mut preset = BuildingPresetDescriptor::from_asset(
            "test-building".into(),
            "旋转测试建筑".into(),
            before.asset_path.clone(),
                before.asset_target.clone(),
        );
        if !accepted {
            preset.footprint.half_extents = [0.75, 3.0];
        } // 原朝向在界内，旋转90°后占地越界。
        install_existing_edit_validator(&mut app, preset);
        app.world_mut()
            .resource_mut::<BuildingPlacementControls>()
            .level_terrain = true;
        prepare(&mut app, root, ViewportMode::Rotate, EditorView::Top, false);
        start(&mut app, node, ViewportMode::Rotate);
        let cursor = cursor(&app, ViewportMode::Rotate, delta);
        step(&mut app, node, cursor);
        let candidate = app.world().get::<Transform>(root).unwrap().rotation;
        assert!(
            candidate
                .angle_between(Quat::from_rotation_y(expected_yaw.to_radians()))
                .abs()
                < 0.002
        );
        release(&mut app, node, cursor);
        for _ in 0..3 {
            step(&mut app, node, cursor);
            assert_eq!(
                app.world().get::<Transform>(root).unwrap().rotation,
                candidate
            );
            assert_eq!(draft_record(&app), &before);
            assert!(
                app.world()
                    .resource::<EditorAuthoringSession>()
                    .undo
                    .is_empty()
            );
        }
        app.world_mut().resource_mut::<ValidationPollingEnabled>().0 = true;
        for _ in 0..64 {
            step(&mut app, node, cursor);
            if app
                .world()
                .resource::<PlacementMoveState>()
                .pending_building
                .is_none()
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(
            app.world()
                .resource::<PlacementMoveState>()
                .pending_building
                .is_none()
        );
        let after = draft_record(&app);
        let session = app.world().resource::<EditorAuthoringSession>();
        if accepted {
            assert_eq!(after.yaw_degrees, expected_yaw);
            assert_eq!(
                after.translation,
                [before.translation[0], 2.0, before.translation[2]]
            );
            assert_eq!(after.water_surfaces, before.water_surfaces);
            assert_eq!(session.terrain_revision, 2); // 旋转不隐式联合整平。
            assert_eq!(session.undo.len(), 1);
            assert!(
                matches!(&session.undo[0], EditorCommand::PlacementChanged { before: old, after: new, .. } if old == &before && new == after)
            );
            assert!(app.world().resource::<SceneEditorState>().dirty);
        } else {
            assert_eq!(after, &before);
            assert!(session.undo.is_empty());
            assert_eq!(session.revision, 7);
            assert!(!app.world().resource::<SceneEditorState>().dirty);
        }
        assert_root(&app, root, after);
        if formal_root {
            assert!(session.placement_previews.is_empty());
        }
    }
}
