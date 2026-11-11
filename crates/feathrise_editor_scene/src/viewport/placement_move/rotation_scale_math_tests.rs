//! 实际编辑相机的旋转圆环与中心缩放回归，验证方向、DPI、端视退化和比例边界。

use super::super::{constants::*, rotation::*, scale::*};
use super::*;
use feathrise_scene_content::{MAX_SCENE_PLACEMENT_SCALE, MIN_SCENE_PLACEMENT_SCALE};

#[test]
fn rotation_ring_pick_and_relative_yaw_use_actual_projection() {
    for view in [EditorView::Top, EditorView::Perspective] {
        for orthographic in [false, true] {
            for size in [
                UVec2::new(1920, 1080),
                UVec2::new(320, 800),
                UVec2::new(1200, 280),
            ] {
                for scale in [1.0, 1.5, 2.0] {
                    let camera = camera(view, orthographic, size, 1.0 / scale);
                    let origin = Vec3::new(4.0, 2.0, -3.0);
                    let ring = rotation_ring(camera, origin).unwrap();
                    assert!(!pick_rotation(
                        camera,
                        origin,
                        camera.project(origin).unwrap()
                    ));
                    let radius = ring.world[0].distance(origin);
                    for angle in [0.0, 45.0, 135.0, 225.0] {
                        let grabbed = origin + yaw_direction(angle) * radius;
                        let cursor = camera.project(grabbed).unwrap();
                        assert!(pick_rotation(camera, origin, cursor));
                        let start = handle_point(
                            camera.ray(cursor).unwrap(),
                            origin,
                            MoveHandle::RotationY,
                        )
                        .unwrap();
                        let cursor = camera
                            .project(origin + yaw_direction(angle + 23.0) * radius)
                            .unwrap();
                        let current = handle_point(
                            camera.ray(cursor).unwrap(),
                            origin,
                            MoveHandle::RotationY,
                        )
                        .unwrap();
                        let yaw = rotation_yaw(37.0, origin, start, current, 0.0).unwrap();
                        assert!((yaw - 60.0).abs() < 0.05);
                        assert_eq!(rotation_yaw(37.0, origin, start, current, 15.0), Some(60.0));
                    }
                }
            }
        }
    }
}

#[test]
fn rotation_rejects_end_on_views_and_axis_center_and_wraps_both_directions() {
    for view in [EditorView::Front, EditorView::Side] {
        for orthographic in [false, true] {
            let camera = camera(view, orthographic, UVec2::new(800, 600), 1.0);
            assert!(rotation_ring(camera, Vec3::ZERO).is_none());
            assert!(!pick_rotation(camera, Vec3::ZERO, Vec2::ZERO));
        }
    }
    assert!(rotation_yaw(0.0, Vec3::ZERO, Vec3::X, Vec3::ZERO, 0.0).is_none());
    assert_eq!(
        rotation_yaw(
            0.0,
            Vec3::ZERO,
            yaw_direction(175.0),
            yaw_direction(-165.0),
            0.0
        )
        .unwrap()
        .round(),
        20.0
    );
    assert_eq!(
        rotation_yaw(0.0, Vec3::ZERO, Vec3::X, yaw_direction(-100.0), 90.0),
        Some(270.0)
    );
    assert_eq!(
        rotation_yaw(0.0, Vec3::ZERO, Vec3::X, yaw_direction(200.0), 90.0),
        Some(180.0)
    );
    for angle in [-100.0, 23.0, 350.0] {
        assert!(
            yaw_direction(angle).distance(Quat::from_rotation_y(angle.to_radians()) * Vec3::X)
                < 0.001
        );
    }
}

#[test]
fn scale_square_keeps_logical_size_in_every_view_projection_and_dpi() {
    for view in [
        EditorView::Top,
        EditorView::Perspective,
        EditorView::Front,
        EditorView::Side,
    ] {
        for orthographic in [false, true] {
            for scale in [0.6, 1.0, 2.0] {
                let camera = camera(view, orthographic, UVec2::new(1200, 280), 1.0 / scale);
                let origin = Vec3::ZERO;
                let center = camera.project(origin).unwrap();
                assert!(pick_scale(camera, origin, center));
                assert!(!pick_scale(
                    camera,
                    origin,
                    center + Vec2::X * 12.0 / camera.size / camera.inverse_scale
                ));
                for point in camera.screen_square(origin, PICK_LOGICAL_PIXELS).unwrap() {
                    let offset = (camera.project(point).unwrap() - center)
                        * camera.size
                        * camera.inverse_scale;
                    assert!(
                        (offset.abs() - Vec2::splat(PICK_LOGICAL_PIXELS))
                            .abs()
                            .max_element()
                            < 0.02
                    );
                }
            }
        }
    }
}

#[test]
fn uniform_scaling_keeps_original_ratios_and_clamps_one_common_factor() {
    let before = [1.1, 1.2, 1.3];
    assert_eq!(
        uniform_scale(before, Vec2::new(120.0, 0.0), false),
        Some([2.2, 2.4, 2.6])
    );
    assert_eq!(
        uniform_scale(before, Vec2::new(0.0, -120.0), false),
        Some([2.2, 2.4, 2.6])
    );
    let value = uniform_scale(before, Vec2::new(1.26_f32.log2() * 120.0, 0.0), true).unwrap();
    for i in 0..3 {
        assert!((value[i] / before[i] - 1.3).abs() < 0.001);
    }
    for delta in [Vec2::splat(-10000.0).with_y(0.0), Vec2::new(10000.0, 0.0)] {
        let value = uniform_scale(before, delta, false).unwrap();
        let factor = value[0] / before[0];
        for i in 0..3 {
            assert!(value[i] >= MIN_SCENE_PLACEMENT_SCALE && value[i] <= MAX_SCENE_PLACEMENT_SCALE);
            assert!((value[i] / before[i] - factor).abs() <= factor * 0.00001);
        }
    }
    assert!(uniform_scale([0.0, 1.0, 1.0], Vec2::ZERO, false).is_none());
    assert!(uniform_scale(before, Vec2::splat(f32::NAN), false).is_none());
}
