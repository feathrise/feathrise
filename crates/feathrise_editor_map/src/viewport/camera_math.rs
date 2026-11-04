//! 编辑器四种视图共享的地图三维取景、相机姿态与正交裁剪纯数学。

use bevy::camera::{OrthographicProjection, ScalingMode}; // 构造遵循Bevy三维惯例的正交投影。
use bevy::prelude::*; // Vec2/Vec3、Transform及默认值。

use crate::types::EditorView; // 方向快捷键只提供轨道角度，不决定投影。

pub(super) const MIN_EXTENT_METERS: f32 = 8.0; // 最大缩放仍保留可操作范围。
pub(super) const MAX_EXTENT_METERS: f32 = 20_000.0; // 常规滚轮上限；显式全图取景可按有限地图范围提高上限。
pub(super) const PERSPECTIVE_FOV_RADIANS: f32 = 50.0_f32.to_radians(); // 两种投影按同一FOV保持焦点平面的覆盖尺寸。
const ORTHOGRAPHIC_NEAR_METERS: f32 = 0.1; // 3D正交相机只渲染前方；禁止用负近裁剪掩盖错误相机位置。
const SCENE_DEPTH_MARGIN_METERS: f32 = 64.0; // 地形量程之外继续容纳树冠、岩石、边界墙和作者辅助图形。

pub(super) fn perspective_transform(center: Vec3, extent: f32, yaw: f32, pitch: f32) -> Transform {
    let distance = extent.max(MIN_EXTENT_METERS);
    let rotation = Quat::from_rotation_y(yaw) * Quat::from_rotation_x(-pitch);
    Transform {
        translation: center + rotation * Vec3::Z * distance,
        rotation,
        ..default()
    } // 直接组合水平与俯仰姿态，正上方也有确定的屏幕上方向，跨极点不再触发looking_at退化或翻转。
}

/// 精确对准方向，之后可继续自由旋转；自由按钮返回默认斜向观察角。
pub(super) fn orbit_angles_from_view(view: EditorView) -> (f32, f32) {
    match view {
        EditorView::Top => (0.0, std::f32::consts::FRAC_PI_2),
        EditorView::Front => (0.0, 0.0),
        EditorView::Side => (std::f32::consts::FRAC_PI_2, 0.0),
        EditorView::Perspective => (-0.72, 0.64),
    }
}

/// 共用轨道距离对应的焦点平面纵向覆盖；切投影时中心平面上的对象大小保持一致。
pub(super) fn focus_plane_height(extent: f32) -> f32 {
    2.0 * (PERSPECTIVE_FOV_RADIANS * 0.5).tan() * extent.max(MIN_EXTENT_METERS)
}

/// 计算地图三维量程中心及AABB半尺寸；固定场景余量只在构造具体观察轴时增加。
pub(super) fn map_scene_frame(
    horizontal_center: Vec2,
    horizontal_size: Vec2,
    base_height: f32,
    height_scale: f32,
) -> (Vec3, Vec3) {
    let terrain_end = base_height + height_scale; // 同时兼容外部旧清单可能使用的反向高度量程。
    let lower_y = base_height.min(terrain_end);
    let upper_y = base_height.max(terrain_end);
    let scene_center = Vec3::new(
        horizontal_center.x,
        (lower_y + upper_y) * 0.5,
        horizontal_center.y,
    );
    let half_extents = Vec3::new(
        horizontal_size.x.abs() * 0.5,
        (upper_y - lower_y) * 0.5,
        horizontal_size.y.abs() * 0.5,
    );
    (scene_center, half_extents)
}

/// 把地图AABB投影到实际观察轴，正交自由旋转也始终覆盖完整深度。
fn orthographic_half_depth(yaw: f32, pitch: f32, scene_half_extents: Vec3) -> f32 {
    let rotation = Quat::from_rotation_y(yaw) * Quat::from_rotation_x(-pitch);
    let direction = rotation * Vec3::Z;
    let map_half_depth = direction.abs().dot(scene_half_extents.abs()); // 顶视时仍只取高度量程，避免大地图对角线污染。
    map_half_depth.max(0.0) + SCENE_DEPTH_MARGIN_METERS
}

/// 构造使用正近裁剪并完整覆盖当前观察轴AABB深度的Bevy 3D正交投影。
pub(super) fn orthographic_projection(
    viewport_height: f32,
    yaw: f32,
    pitch: f32,
    scene_half_extents: Vec3,
) -> OrthographicProjection {
    let half_depth = orthographic_half_depth(yaw, pitch, scene_half_extents);
    OrthographicProjection {
        scaling_mode: ScalingMode::FixedVertical { viewport_height },
        near: ORTHOGRAPHIC_NEAR_METERS,
        far: half_depth * 2.0 + 1.0,
        ..OrthographicProjection::default_3d()
    }
}

pub(super) fn orthographic_transform(
    center: Vec3,
    scene_center: Vec3,
    scene_half_extents: Vec3,
    yaw: f32,
    pitch: f32,
) -> Transform {
    let rotation = Quat::from_rotation_y(yaw) * Quat::from_rotation_x(-pitch);
    let direction = rotation * Vec3::Z;
    let distance = (scene_center - center).dot(direction)
        + orthographic_half_depth(yaw, pitch, scene_half_extents);
    Transform {
        translation: center + direction * distance,
        rotation,
        ..default()
    } // 只修正观察轴深度；屏幕内平移与焦点保持一致，近裁剪前方始终容纳当前地图。
}

pub(super) fn navigation_axes(yaw: f32, pitch: f32) -> (Vec3, Vec3) {
    let transform = perspective_transform(Vec3::ZERO, 1.0, yaw, pitch);
    (transform.rotation * Vec3::X, transform.rotation * Vec3::Y)
}

/// 固定八角投影取景，透视包含近侧深度，正交只使用屏幕宽高；窄视口也容纳完整范围。
pub(crate) fn framing_extent(
    half_extents: Vec3, yaw: f32, pitch: f32, render_size: UVec2, orthographic: bool,
) -> f32 {
    let half_extents = half_extents.abs();
    let rotation = Quat::from_rotation_y(yaw) * Quat::from_rotation_x(-pitch);
    let inverse = rotation.inverse();
    let tangent = (PERSPECTIVE_FOV_RADIANS * 0.5).tan();
    let aspect = render_size.x.max(1) as f32 / render_size.y.max(1) as f32;
    let mut extent = MIN_EXTENT_METERS;
    for x in [-1.0, 1.0] {
        for y in [-1.0, 1.0] {
            for z in [-1.0, 1.0] {
                let corner = inverse * (half_extents * Vec3::new(x, y, z));
                let screen_distance = (corner.y.abs() / tangent).max(corner.x.abs() / (tangent * aspect));
                let required = screen_distance + if orthographic { 0.0 } else { corner.z };
                extent = extent.max(required);
            }
        }
    }
    (extent * 1.12).max(MIN_EXTENT_METERS) // 四周留12%余量；大图在窄视口不能被常规滚轮上限裁掉。
}

#[cfg(test)]
#[path = "camera_math_tests.rs"]
mod tests;
