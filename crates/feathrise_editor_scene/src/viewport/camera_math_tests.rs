// 场景三维取景、正交深度与固定视图姿态的纯算法回归测试。
use super::*;

#[test]
fn perspective_orientation_is_finite_and_continuous_at_both_poles() {
    for pole in [-std::f32::consts::FRAC_PI_2, std::f32::consts::FRAC_PI_2] {
        let before = perspective_transform(Vec3::ZERO, 100.0, 0.4, pole - 0.001);
        let at = perspective_transform(Vec3::ZERO, 100.0, 0.4, pole);
        let after = perspective_transform(Vec3::ZERO, 100.0, 0.4, pole + 0.001);
        assert!(at.rotation.is_finite() && at.translation.is_finite());
        assert!(before.rotation.dot(at.rotation).abs() > 0.99999);
        assert!(at.rotation.dot(after.rotation).abs() > 0.99999); // 小幅跨极点不允许产生180度的上方向翻转。
        assert!(at.forward().dot(-at.translation.normalize()) > 0.99999);
    }
}

#[test]
fn fixed_views_seed_the_same_screen_axes_before_orbiting() {
    for view in [EditorView::Top, EditorView::Front, EditorView::Side] {
        let (yaw, pitch) = orbit_angles_from_view(view);
        let fixed = orthographic_transform(Vec3::ZERO, Vec3::ZERO, Vec3::ONE, yaw, pitch);
        let orbit = perspective_transform(Vec3::ZERO, 100.0, yaw, pitch);
        assert!(fixed.rotation.dot(orbit.rotation).abs() > 0.99999); // 起转沿当前屏幕方向接续，不能套用上次透视角度。
    }
}

#[test]
fn orbit_wrap_keeps_the_same_pose_after_a_full_turn() {
    let before = perspective_transform(Vec3::new(10.0, 20.0, 30.0), 120.0, 0.8, -2.9);
    let after = perspective_transform(
        Vec3::new(10.0, 20.0, 30.0),
        120.0,
        0.8,
        -2.9 + std::f32::consts::TAU,
    );
    assert!(before.rotation.dot(after.rotation).abs() > 0.99999);
    assert!(before.translation.distance(after.translation) < 0.001);
}

#[test]
fn top_view_screen_right_and_up_match_world_x_and_negative_z() {
    let (yaw, pitch) = orbit_angles_from_view(EditorView::Top);
    let (right, up) = navigation_axes(yaw, pitch);
    assert!(right.dot(Vec3::X) > 0.999);
    assert!(up.dot(Vec3::NEG_Z) > 0.999);
}

#[test] // 64米新场景的水平视野小于160米高度量程，顶视深度不能再借用视野extent。
fn top_view_stays_above_the_full_new_scene_height_range_after_panning_and_zooming() {
    let (scene_center, scene_half_extents) =
        scene_scene_frame(Vec2::ZERO, Vec2::splat(64.0), -80.0, 160.0);
    let navigated_center = Vec3::new(8.0, -4_000.0, -12.0); // 模拟透视视图曾经大幅垂直平移共享观察中心。
    let (yaw, pitch) = orbit_angles_from_view(EditorView::Top);
    let transform = orthographic_transform(
        navigated_center,
        scene_center,
        scene_half_extents,
        yaw,
        pitch,
    );
    let projection = orthographic_projection(
        focus_plane_height(MIN_EXTENT_METERS), yaw, pitch, scene_half_extents,
    ); // 模拟滚轮缩到允许的最小视野。
    assert!(transform.translation.y > 80.0); // 相机必须位于R16最高可表示地形之上。
    assert!(transform.forward().dot(Vec3::NEG_Y) > 0.999); // 顶视严格沿世界负Y观察。
    for terrain_y in [-80.0, 0.0, 80.0] {
        let world_point = Vec3::new(navigated_center.x, terrain_y, navigated_center.z);
        let view_point = transform.rotation.inverse() * (world_point - transform.translation);
        let forward_distance = -view_point.z; // Bevy三维相机沿局部负Z观察。
        assert!(forward_distance > projection.near);
        assert!(forward_distance < projection.far);
    }
}

#[test] // 数公里水平范围只改变顶视覆盖尺寸，不能再把相机沿Y抬到场景对角线距离。
fn top_view_depth_is_independent_from_large_horizontal_scene_dimensions() {
    let (small_center, small_half_extents) =
        scene_scene_frame(Vec2::ZERO, Vec2::splat(64.0), -80.0, 160.0);
    let (large_center, large_half_extents) =
        scene_scene_frame(Vec2::ZERO, Vec2::splat(8_192.0), -80.0, 160.0);
    let (yaw, pitch) = orbit_angles_from_view(EditorView::Top);
    let small = orthographic_transform(
        small_center,
        small_center,
        small_half_extents,
        yaw,
        pitch,
    );
    let large = orthographic_transform(
        large_center,
        large_center,
        large_half_extents,
        yaw,
        pitch,
    );
    assert!((small.translation.y - large.translation.y).abs() < 0.001); // 两张图共享相同高度量程和64米场景余量。
    assert!((large.translation.y - 144.0).abs() < 0.001); // -80..80量程的最高点80米，上方再留64米。
}

#[test]
fn freely_rotated_orthographic_camera_keeps_all_scene_corners_inside_depth_range() {
    let scene_center = Vec3::new(-100.0, 20.0, 300.0);
    let half_extents = Vec3::new(500.0, 80.0, 200.0);
    let center = Vec3::new(-900.0, -4_000.0, 1_500.0); // 焦点远离场景仍不能污染沿观察轴的完整裁剪。
    for (yaw, pitch) in [(0.8, -0.6), (-1.4, 2.1), (3.0, -2.8)] {
        let transform = orthographic_transform(center, scene_center, half_extents, yaw, pitch);
        let projection = orthographic_projection(focus_plane_height(8.0), yaw, pitch, half_extents);
        for x in [-1.0, 1.0] {
            for y in [-1.0, 1.0] {
                for z in [-1.0, 1.0] {
                    let corner = scene_center + half_extents * Vec3::new(x, y, z);
                    let view_point = transform.rotation.inverse() * (corner - transform.translation);
                    assert!(-view_point.z > projection.near);
                    assert!(-view_point.z < projection.far); // 旋转到斜向、越过极点后八个角仍全部处于正近裁剪前方。
                }
            }
        }
    }
}
