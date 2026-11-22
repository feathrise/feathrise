//! 固定三轴和平面的绘制与拾取共用实际编辑相机，不扫描场景。

use super::super::systems::editor_camera_pose;
use super::types::{MoveAxis, MoveHandle, MovePlane};
use crate::resources::{SceneEditorState, SceneEditorViewportState};
use bevy::prelude::*;
use feathrise_editor_animation_model::gizmo::GizmoCamera;

const PICK_LOGICAL_PIXELS: f32 = 8.0;
const PLANE_INNER_FRACTION: f32 = feathrise_author_tools::AUTHOR_PLANE_INNER_FRACTION;
const PLANE_OUTER_FRACTION: f32 = feathrise_author_tools::AUTHOR_PLANE_OUTER_FRACTION; // 约26逻辑像素的方框，靠近原点但避开两根轴。

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MoveCamera(GizmoCamera);

impl std::ops::Deref for MoveCamera {
    type Target = GizmoCamera;

    fn deref(&self) -> &GizmoCamera {
        &self.0
    }
}

impl MoveCamera {
    pub(crate) fn from_projection(
        projection: &Projection,
        transform: &GlobalTransform,
        size: Vec2,
        inverse_scale: f32,
    ) -> Option<Self> {
        GizmoCamera::from_projection(projection, transform, size, inverse_scale).map(Self)
    }

    pub(crate) fn new(
        editor: &SceneEditorState,
        viewport: &SceneEditorViewportState,
        inverse_scale: f32,
    ) -> Option<Self> {
        let size = viewport.render_size.as_vec2();
        if !size.is_finite()
            || size.min_element() < 1.0
            || !inverse_scale.is_finite()
            || inverse_scale <= 0.0
        {
            return None;
        }
        let (projection, transform) = editor_camera_pose(editor.orthographic, viewport);
        GizmoCamera::from_view(projection, transform.to_matrix(), size, inverse_scale).map(Self)
    }

    /// 绘制和拾取共用同一四角与退化门；侧着看时不能保留难以点击的细线平面。
    pub(crate) fn plane_quad(self, origin: Vec3, length: f32) -> Option<[Vec2; 4]> {
        self.project_quad(plane_corners(origin, length))
    }

    pub(super) fn project_quad(self, corners: [Vec3; 4]) -> Option<[Vec2; 4]> {
        let mut quad = [Vec2::ZERO; 4];
        for (point, corner) in quad.iter_mut().zip(corners) {
            *point = self.logical_point(corner)?;
        }
        let mut twice_area = 0.0_f32;
        let mut longest_edge = 0.0_f32;
        for i in 0..4 {
            twice_area += quad[i].perp_dot(quad[(i + 1) % 4]);
            longest_edge = longest_edge.max(quad[i].distance(quad[(i + 1) % 4]));
        }
        (longest_edge >= 8.0 && twice_area.abs() * 0.5 / longest_edge >= 8.0).then_some(quad) // 面积/最长边近似可点击厚度，按逻辑像素固定下限，兼容正交和DPI。
    }

    pub(crate) fn pick_handle(
        self,
        origin: Vec3,
        normalized: Vec2,
        building: bool,
    ) -> Option<MoveHandle> {
        self.pick_move_planes(origin, normalized, building, &[MovePlane::Xz])
    }

    pub(super) fn pick_mesh_move(self, origin: Vec3, normalized: Vec2, building: bool) -> Option<MoveHandle> {
        let planes = if building { &MovePlane::ALL[..1] } else { &MovePlane::ALL };
        self.pick_move_planes(origin, normalized, building, planes)
    }

    fn pick_move_planes(self, origin: Vec3, normalized: Vec2, building: bool, planes: &[MovePlane]) -> Option<MoveHandle> {
        if !normalized.is_finite() {
            return None;
        }
        let length = self.handle_length(origin)?;
        let cursor = normalized * self.size * self.inverse_scale;
        if let Some(plane) = planes.iter().copied().find(|plane| {
            self.project_quad(move_plane_corners(origin, length, *plane))
                .is_some_and(|quad| quad_contains(quad, cursor))
        }) {
            return Some(plane.move_handle()); // 实际方框内部优先于邻轴的宽拾取容差。
        }
        self.pick_axis(origin, normalized, building)
            .map(MoveHandle::Axis)
    }

    pub(super) fn logical_point(self, point: Vec3) -> Option<Vec2> {
        Some(self.project(point)? * self.size * self.inverse_scale)
    }

    /// 单轴作者对象只拾取实际画出的轴，不能让隐藏的XYZ或XZ平面抢走水位手柄。
    pub(crate) fn picks_axis(self, origin: Vec3, normalized: Vec2, axis: MoveAxis) -> bool {
        let Some(length) = self.handle_length(origin) else { return false; };
        let Some((start, end)) = self.axis_segment(origin, axis, length) else { return false; };
        let segment = end - start;
        let cursor = normalized * self.size * self.inverse_scale;
        let t = (cursor - start).dot(segment) / segment.length_squared();
        normalized.is_finite()
            && (12.0 / segment.length()..=1.08).contains(&t)
            && cursor.distance(start + segment * t.clamp(0.0, 1.0)) <= PICK_LOGICAL_PIXELS
    }

    fn pick_axis(self, origin: Vec3, normalized: Vec2, building: bool) -> Option<MoveAxis> {
        let length = self.handle_length(origin)?;
        let cursor = normalized * self.size * self.inverse_scale;
        let mut nearest = None;
        for axis in MoveAxis::ALL {
            if building && axis == MoveAxis::Y {
                continue;
            }
            let Some((start, end)) = self.axis_segment(origin, axis, length) else {
                continue;
            };
            let segment = end - start;
            let t = (cursor - start).dot(segment) / segment.length_squared();
            if t < 12.0 / segment.length() || t > 1.08 {
                continue;
            } // 原点留出空隙，避免三轴交点抢走普通选择。
            let distance = cursor.distance(start + segment * t.clamp(0.0, 1.0));
            if distance <= PICK_LOGICAL_PIXELS && nearest.is_none_or(|(_, best)| distance < best) {
                nearest = Some((axis, distance));
            }
        }
        nearest.map(|(axis, _)| axis)
    }
}

pub(crate) fn plane_corners(origin: Vec3, length: f32) -> [Vec3; 4] {
    move_plane_corners(origin, length, MovePlane::Xz)
}

pub(super) fn move_plane_corners(origin: Vec3, length: f32, plane: MovePlane) -> [Vec3; 4] {
    plane_corners_along(origin, plane.axes().map(MoveAxis::direction), length * PLANE_INNER_FRACTION, length * PLANE_OUTER_FRACTION)
}

pub(super) fn plane_corners_along(origin: Vec3, [first, second]: [Vec3; 2], inner: f32, outer: f32) -> [Vec3; 4] {
    [
        origin + first * inner + second * inner,
        origin + first * outer + second * inner,
        origin + first * outer + second * outer,
        origin + first * inner + second * outer,
    ]
}

pub(super) fn quad_contains(quad: [Vec2; 4], cursor: Vec2) -> bool {
    let mut positive = false;
    let mut negative = false;
    for index in 0..4 {
        let side = (quad[(index + 1) % 4] - quad[index]).perp_dot(cursor - quad[index]);
        positive |= side > 0.001;
        negative |= side < -0.001;
    }
    !(positive && negative)
}

/// 平面冻结在按下的模型根，交点差只传给两个允许轴。
pub(crate) fn handle_point(ray: Ray3d, origin: Vec3, handle: MoveHandle) -> Option<Vec3> {
    match handle {
        MoveHandle::Axis(axis) => {
            axis_scalar(ray, origin, axis).map(|scalar| origin + axis.direction() * scalar)
        }
        MoveHandle::XyPlane => plane_point(ray, origin, MoveAxis::Z),
        MoveHandle::XzPlane | MoveHandle::RotationY => plane_point(ray, origin, MoveAxis::Y),
        MoveHandle::YzPlane => plane_point(ray, origin, MoveAxis::X),
        _ => Some(origin), // 通用旋转环和缩放直接处理屏幕光标。
    }
}

fn plane_point(ray: Ray3d, origin: Vec3, normal: MoveAxis) -> Option<Vec3> {
    let index = normal.index();
    let direction = ray.direction[index];
    if direction.abs() <= 0.0001 {
        return None;
    }
    let distance = (origin[index] - ray.origin[index]) / direction;
    if !distance.is_finite() || distance < 0.0 {
        return None;
    }
    let mut point = ray.get_point(distance);
    point[index] = origin[index];
    point.is_finite().then_some(point)
}

pub(crate) fn handle_translation(
    origin: Vec3,
    delta: Vec3,
    handle: MoveHandle,
    grid: f32,
    bounds_min: Vec2,
    bounds_max: Vec2,
) -> Option<Vec3> {
    handle_translation_mode(origin, delta, handle, grid, feathrise_editor_core::EditorSnapMode::WorldGrid, bounds_min, bounds_max)
}

/// 固定步长以按下时原点为相位，网格点以世界零点为相位；容器不裁切有限世界位置。
pub(crate) fn handle_translation_mode(
    origin: Vec3, delta: Vec3, handle: MoveHandle, grid: f32,
    mode: feathrise_editor_core::EditorSnapMode, _bounds_min: Vec2, _bounds_max: Vec2,
) -> Option<Vec3> {
    let mut position = origin;
    for axis in MoveAxis::ALL {
        if !handle.moves_axis(axis) {
            continue;
        }
        let start = origin[axis.index()];
        let phase = if mode == feathrise_editor_core::EditorSnapMode::Relative { start } else { 0.0 };
        position[axis.index()] = axis_coordinate(start - phase, delta[axis.index()], grid, f32::NEG_INFINITY, f32::INFINITY)? + phase;
    } // 只对所选轴各自吸附，不补偿未选轴。
    position.is_finite().then_some(position)
}

/// 求鼠标射线到世界轴的最近点参数；平行轴必须拒绝，不能放大为极端位移。
pub(crate) fn axis_scalar(ray: Ray3d, origin: Vec3, axis: MoveAxis) -> Option<f32> {
    let a = axis.direction();
    let d = *ray.direction;
    let w = ray.origin - origin;
    let ad = a.dot(d);
    let denominator = 1.0 - ad * ad;
    if denominator <= 0.0001 {
        return None;
    }
    let value = (a.dot(w) - ad * d.dot(w)) / denominator;
    value.is_finite().then_some(value)
}

pub(crate) fn axis_coordinate(
    start: f32,
    delta: f32,
    grid: f32,
    min: f32,
    max: f32,
) -> Option<f32> {
    let mut value = start + delta;
    if !value.is_finite() {
        return None;
    }
    if grid.is_finite() && grid > 0.0 {
        value = (value / grid).round() * grid;
    }
    value.is_finite().then_some(value.clamp(min, max)) // 边界优先于吸附，禁止用邻轴补偿越界。
}
