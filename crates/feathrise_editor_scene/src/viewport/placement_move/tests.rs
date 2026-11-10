//! 用实际编辑相机验证各方向/投影的拾取、约束和缩放，不启动窗口或第二个 View。

use super::{
    math::{
        MoveCamera, axis_coordinate, axis_scalar, handle_point, handle_translation, plane_corners,
    },
    types::{MoveAxis, MoveHandle},
};
use crate::{
    resources::{SceneEditorState, SceneEditorViewportState},
    types::EditorView,
    viewport::camera_math::orbit_angles_from_view,
};
use bevy::prelude::*;

fn camera(view: EditorView, orthographic: bool, size: UVec2, inverse_scale: f32) -> MoveCamera {
    let editor = SceneEditorState {
        view,
        orthographic,
        ..default()
    };
    let (yaw, pitch) = orbit_angles_from_view(view);
    MoveCamera::new(
        &editor,
        &SceneEditorViewportState {
            _image: Handle::default(),
            camera: Entity::PLACEHOLDER,
            center: Vec3::ZERO,
            scene_center: Vec3::ZERO,
            scene_half_extents: Vec3::new(300.0, 40.0, 300.0),
            extent: 120.0,
            yaw,
            pitch,
            framed_scene_id: None,
            render_size: size,
        },
        inverse_scale,
    )
    .unwrap()
}

#[test]
fn semantic_scale_preserves_actual_screen_square_in_all_projections() {
    use feathrise_author_tools::{AuthorTool, AuthorToolPreview, TransformPreview};
    for view in [EditorView::Perspective, EditorView::Top, EditorView::Front, EditorView::Side] {
        for orthographic in [false, true] {
            for size in [UVec2::new(1920, 1080), UVec2::new(320, 800), UVec2::new(1200, 280)] {
                for scale in [1.0, 1.5, 2.0] {
                    let camera = camera(view, orthographic, size, 1.0 / scale);
                    let origin = Vec3::new(1.75, 3.0, -4.0);
                    let original = camera.screen_square(origin, super::constants::PICK_LOGICAL_PIXELS).unwrap();
                    let preview = super::render::screen_scale_preview(origin, original, Color::WHITE).unwrap();
                    AuthorToolPreview { items: vec![AuthorTool::Transform(preview.clone())] }.validate().unwrap();
                    let TransformPreview::Scale { origin, right, up, half_size, cube, .. } = preview else { unreachable!() };
                    assert!(!cube);
                    let center = Vec3::from_array(origin);
                    let right = Vec3::from_array(right) * half_size;
                    let up = Vec3::from_array(up) * half_size;
                    let restored = [center - right - up, center + right - up, center + right + up, center - right + up];
                    for (before, after) in original.into_iter().zip(restored) {
                        assert!(before.distance(after) < half_size * 0.002 + 0.0002, "{view:?}, ortho={orthographic}, scale={scale}");
                    }
                }
            }
        }
    }
}

#[test]
fn semantic_move_keeps_hidden_axis_and_building_height_rules() {
    for orthographic in [false, true] {
        for (view, hidden) in [(EditorView::Top, MoveAxis::Y), (EditorView::Front, MoveAxis::Z), (EditorView::Side, MoveAxis::X)] {
            let camera = camera(view, orthographic, UVec2::new(800, 600), 1.0);
            let origin = Vec3::ZERO;
            let length = camera.handle_length(origin).unwrap();
            let ordinary = super::render::visible_axes(camera, origin, length, 0b111);
            let building = super::render::visible_axes(camera, origin, length, 0b101);
            assert_eq!(ordinary & (1 << hidden.index()), 0);
            assert_eq!(building & 0b010, 0);
            assert_eq!(building, ordinary & 0b101);
        }
    }
}

#[test]
fn actual_projection_picks_each_visible_axis_and_reconstructs_world_motion() {
    for view in [
        EditorView::Perspective,
        EditorView::Top,
        EditorView::Front,
        EditorView::Side,
    ] {
        for orthographic in [false, true] {
            for size in [
                UVec2::new(1920, 1080),
                UVec2::new(320, 800),
                UVec2::new(1200, 280),
            ] {
                for scale in [1.0, 1.5, 2.0] {
                    let camera = camera(view, orthographic, size, 1.0 / scale);
                    let origin = Vec3::ZERO;
                    let length = camera.handle_length(origin).unwrap();
                    for axis in MoveAxis::ALL {
                        if camera.axis_segment(origin, axis, length).is_none() {
                            continue;
                        }
                        let point = origin + axis.direction() * length * 0.65;
                        let cursor = camera.project(point).unwrap();
                        assert_eq!(
                            camera.pick_handle(origin, cursor, false),
                            Some(MoveHandle::Axis(axis)),
                            "{view:?}, ortho={orthographic}, {axis:?}"
                        );
                        let scalar =
                            axis_scalar(camera.ray(cursor).unwrap(), origin, axis).unwrap();
                        assert!((scalar - length * 0.65).abs() < length * 0.001 + 0.002);
                    }
                    assert_eq!(
                        camera.pick_handle(origin, camera.project(origin).unwrap(), false),
                        None
                    );
                }
            }
        }
    }
}

#[test]
fn end_on_axis_is_hidden_and_buildings_have_no_vertical_handle() {
    for orthographic in [false, true] {
        for (view, hidden) in [
            (EditorView::Top, MoveAxis::Y),
            (EditorView::Front, MoveAxis::Z),
            (EditorView::Side, MoveAxis::X),
        ] {
            let camera = camera(view, orthographic, UVec2::new(800, 600), 1.0);
            assert!(
                camera
                    .axis_segment(
                        Vec3::ZERO,
                        hidden,
                        camera.handle_length(Vec3::ZERO).unwrap()
                    )
                    .is_none()
            );
        }
        let camera = camera(
            EditorView::Perspective,
            orthographic,
            UVec2::new(800, 600),
            1.0,
        );
        let cursor = camera
            .project(Vec3::Y * camera.handle_length(Vec3::ZERO).unwrap() * 0.65)
            .unwrap();
        assert_eq!(camera.pick_handle(Vec3::ZERO, cursor, true), None);
    }
}

#[test]
fn grid_rounds_world_coordinate_then_clamps_without_touching_other_axes() {
    assert_eq!(axis_coordinate(0.25, 2.36, 1.0, -10.0, 10.0), Some(3.0));
    assert_eq!(axis_coordinate(-0.25, -2.36, 1.0, -10.0, 10.0), Some(-3.0));
    assert_eq!(axis_coordinate(0.25, 2.36, 0.0, -10.0, 10.0), Some(2.61));
    assert_eq!(axis_coordinate(0.0, 20.0, 1.0, -3.2, 3.2), Some(3.2));
    assert_eq!(axis_coordinate(0.0, f32::NAN, 1.0, -3.2, 3.2), None);
    assert!(axis_scalar(Ray3d::new(Vec3::ZERO, Dir3::X), Vec3::ZERO, MoveAxis::X).is_none());
}

#[test]
fn plane_pick_and_grab_offset_use_actual_projection_across_sizes_and_dpi() {
    for view in [EditorView::Perspective, EditorView::Top] {
        for orthographic in [false, true] {
            for size in [
                UVec2::new(1920, 1080),
                UVec2::new(320, 800),
                UVec2::new(1200, 280),
            ] {
                for scale in [1.0, 1.5, 2.0] {
                    let camera = camera(view, orthographic, size, 1.0 / scale);
                    let origin = Vec3::new(4.0, 2.0, -3.0);
                    let length = camera.handle_length(origin).unwrap();
                    assert!(camera.plane_quad(origin, length).is_some());
                    let corners = plane_corners(origin, length);
                    let grabbed = corners[0].lerp(corners[2], 0.5);
                    let cursor = camera.project(grabbed).unwrap();
                    for building in [false, true] {
                        assert_eq!(
                            camera.pick_handle(origin, cursor, building),
                            Some(MoveHandle::XzPlane)
                        );
                    }
                    let start =
                        handle_point(camera.ray(cursor).unwrap(), origin, MoveHandle::XzPlane)
                            .unwrap();
                    let delta = Vec3::new(2.25, 0.0, -1.37);
                    let cursor = camera.project(grabbed + delta).unwrap();
                    let current =
                        handle_point(camera.ray(cursor).unwrap(), origin, MoveHandle::XzPlane)
                            .unwrap();
                    let position = handle_translation(
                        origin,
                        current - start,
                        MoveHandle::XzPlane,
                        0.0,
                        Vec2::splat(-1000.0),
                        Vec2::splat(1000.0),
                    )
                    .unwrap();
                    assert!(
                        position.distance(origin + delta) < 0.01,
                        "{view:?}, ortho={orthographic}, size={size:?}, scale={scale}"
                    );
                    assert_eq!(position.y, origin.y);
                    assert!(
                        camera
                            .pick_handle(origin, camera.project(origin).unwrap(), false)
                            .is_none()
                    );
                }
            }
        }
    }
}

#[test]
fn edge_on_plane_and_parallel_or_backward_rays_are_rejected() {
    for view in [EditorView::Front, EditorView::Side] {
        for orthographic in [false, true] {
            let camera = camera(view, orthographic, UVec2::new(800, 600), 1.0);
            let length = camera.handle_length(Vec3::ZERO).unwrap();
            assert!(camera.plane_quad(Vec3::ZERO, length).is_none());
            let ray = camera.ray(Vec2::ZERO).unwrap();
            assert!(handle_point(ray, Vec3::ZERO, MoveHandle::XzPlane).is_none());
        }
    }
    assert!(
        handle_point(
            Ray3d::new(Vec3::Y, Dir3::Y),
            Vec3::ZERO,
            MoveHandle::XzPlane
        )
        .is_none()
    );
    assert!(
        handle_point(
            Ray3d::new(Vec3::ZERO, Dir3::X),
            Vec3::ZERO,
            MoveHandle::XzPlane
        )
        .is_none()
    );
}

#[test]
fn plane_grid_applies_to_both_coordinates_without_container_clipping() {
    let position = handle_translation(
        Vec3::new(0.25, 17.5, -0.25),
        Vec3::new(2.36, 100.0, -2.36),
        MoveHandle::XzPlane,
        1.0,
        Vec2::splat(-10.0),
        Vec2::splat(10.0),
    )
    .unwrap();
    assert_eq!(position, Vec3::new(3.0, 17.5, -3.0));
    let moved = handle_translation(
        position,
        Vec3::new(20.0, 0.0, -20.0),
        MoveHandle::XzPlane,
        1.0,
        Vec2::splat(-3.2),
        Vec2::splat(3.2),
    )
    .unwrap();
    assert_eq!(moved, Vec3::new(23.0, 17.5, -23.0));
    assert!(
        handle_translation(
            position,
            Vec3::new(f32::NAN, 0.0, 0.0),
            MoveHandle::XzPlane,
            0.0,
            Vec2::splat(-10.0),
            Vec2::splat(10.0)
        )
        .is_none()
    );
}

#[test]
fn plane_pick_uses_polygon_instead_of_its_screen_bounding_box() {
    let camera = camera(EditorView::Perspective, false, UVec2::new(800, 600), 1.0);
    let origin = Vec3::ZERO;
    let length = camera.handle_length(origin).unwrap();
    let quad = camera.plane_quad(origin, length).unwrap();
    let min = quad.iter().copied().reduce(Vec2::min).unwrap();
    let max = quad.iter().copied().reduce(Vec2::max).unwrap();
    for pixel in [min, max, Vec2::new(min.x, max.y), Vec2::new(max.x, min.y)] {
        let cursor = pixel / camera.size / camera.inverse_scale;
        assert_ne!(
            camera.pick_handle(origin, cursor, false),
            Some(MoveHandle::XzPlane)
        );
    }
}

#[path = "rotation_scale_math_tests.rs"]
mod rotation_scale;
