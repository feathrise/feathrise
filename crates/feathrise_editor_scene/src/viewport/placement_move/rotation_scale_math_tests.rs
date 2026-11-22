//! 实际编辑相机的旋转圆环与中心缩放回归，验证方向、DPI、端视退化和比例边界。

use super::super::{rotation::*, scale::*};
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
fn scale_center_uses_a_round_pick_region_in_every_view_projection_and_dpi() {
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
                let logical = camera.size * camera.inverse_scale;
                let pick = |offset: Vec2| {
                    pick_scale_handle(camera, origin, Mat3::IDENTITY, center + offset / logical, false)
                };
                assert_eq!(pick(Vec2::ZERO), Some(MoveHandle::UniformScale));
                for direction in [Vec2::X, Vec2::Y, Vec2::NEG_X, Vec2::NEG_Y] {
                    assert_eq!(pick(direction * (UNIFORM_SCALE_RADIUS - 1.0)), Some(MoveHandle::UniformScale));
                }
                // 这个对角点仍在原方框以内，已经超出新圆盘。
                assert_ne!(pick(Vec2::splat(UNIFORM_SCALE_RADIUS * 0.8)), Some(MoveHandle::UniformScale));
            }
        }
    }
}

#[test]
fn uniform_scaling_keeps_original_ratios_and_clamps_one_common_factor() {
    let before = [1.1, 1.2, 1.3];
    let camera = camera(EditorView::Perspective, false, UVec2::new(1200, 280), 0.5);
    let origin = Vec3::new(4.0, 2.0, -3.0);
    let point = |offset: Vec2| camera.project(origin).unwrap() + offset / (camera.size * camera.inverse_scale);
    let scale = |start, end, before, step| {
        scale_values(camera, origin, Mat3::IDENTITY, point(start), point(end), MoveHandle::UniformScale, before, step)
    };
    let start = Vec2::new(60.0, 80.0);
    // 不同比例的画布下，各象限到中心的距离同样决定倍率。
    for sign in [Vec2::ONE, -Vec2::ONE, Vec2::new(1.0, -1.0), Vec2::new(-1.0, 1.0)] {
        let start = start * sign;
        for factor in [0.5, 2.0] {
            let value = scale(start, start * factor, before, 0.0).unwrap();
            assert!(Vec3::from_array(value).abs_diff_eq(Vec3::from_array(before) * factor, 0.002));
        }
    }
    assert_eq!(scale(start, Vec2::new(-80.0, 60.0), before, 0.3), Some(before));
    let snapped = scale(start, start * 1.26, before, 0.1).unwrap();
    assert!(Vec3::from_array(snapped).abs_diff_eq(Vec3::from_array(before) * 1.3, 0.002));
    for end in [Vec2::ZERO, start * 10_000.0] {
        let value = scale(start, end, before, 0.0).unwrap();
        let factor = value[0] / before[0];
        for i in 0..3 {
            assert!(value[i] >= MIN_SCENE_PLACEMENT_SCALE && value[i] <= MAX_SCENE_PLACEMENT_SCALE);
            assert!((value[i] / before[i] - factor).abs() <= factor * 0.00001);
        }
    }
    assert_eq!(scale(Vec2::ZERO, Vec2::ZERO, before, 0.0), Some(before));
    let from_center = scale(Vec2::ZERO, Vec2::Y * UNIFORM_SCALE_RADIUS, before, 0.0).unwrap();
    assert!(Vec3::from_array(from_center).abs_diff_eq(Vec3::from_array(before) * 2.0, 0.002));
    assert!(scale(start, Vec2::ZERO, [0.0, 1.0, 1.0], 0.0).is_none());
    assert!(scale(start, Vec2::splat(f32::NAN), before, 0.0).is_none());
}
