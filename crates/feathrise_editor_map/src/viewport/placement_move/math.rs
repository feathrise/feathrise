//! 复用实际编辑相机的矩阵，固定三轴与XZ平面的拾取不依赖地形射线或场景规模。

use super::super::systems::editor_camera_pose;
use super::types::{MoveAxis, MoveHandle};
use crate::resources::{MapEditorState, MapEditorViewportState};
use bevy::prelude::*;

const HANDLE_LOGICAL_PIXELS: f32 = 80.0;
const PICK_LOGICAL_PIXELS: f32 = 8.0;
const PLANE_INNER_FRACTION: f32 = feathrise_author_tools::AUTHOR_PLANE_INNER_FRACTION;
const PLANE_OUTER_FRACTION: f32 = feathrise_author_tools::AUTHOR_PLANE_OUTER_FRACTION; // 约26逻辑像素的方框，靠近原点但避开两根轴。

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MoveCamera {
    clip_from_world: Mat4,
    world_from_clip: Mat4,
    pub size: Vec2,
    pub inverse_scale: f32,
}

impl MoveCamera {
    /// 资产预览复用同一投影数学，只传入其既有相机，不新增地图相机状态。
    pub(crate) fn from_projection(projection: &Projection, transform: &GlobalTransform, size: Vec2, inverse_scale: f32) -> Option<Self> {
        if !size.is_finite() || size.min_element() <= 0.0 { return None; }
        let mut projection=projection.clone();
        projection.update(size.x,size.y);
        let clip_from_world=projection.get_clip_from_view()*transform.to_matrix().inverse();
        let world_from_clip=clip_from_world.inverse();
        (clip_from_world.is_finite() && world_from_clip.is_finite()).then_some(Self { clip_from_world,world_from_clip,size,inverse_scale })
    }
    pub(crate) fn new(
        editor: &MapEditorState,
        viewport: &MapEditorViewportState,
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
        let (mut projection, transform) = editor_camera_pose(editor.orthographic, viewport);
        projection.update(size.x, size.y);
        let clip_from_world = projection.get_clip_from_view() * transform.to_matrix().inverse();
        let world_from_clip = clip_from_world.inverse();
        (clip_from_world.is_finite() && world_from_clip.is_finite()).then_some(Self {
            clip_from_world,
            world_from_clip,
            size,
            inverse_scale,
        })
    }

    pub(crate) fn project(self, world: Vec3) -> Option<Vec2> {
        let clip = self.clip_from_world * world.extend(1.0);
        if !clip.is_finite() || clip.w <= 0.0 {
            return None;
        }
        let ndc = clip.truncate() / clip.w;
        if !(0.0..=1.0).contains(&ndc.z) {
            return None;
        } // 不显示近裁剪后方或正交深度外的手柄。
        Some(Vec2::new(ndc.x, -ndc.y) * 0.5)
    }

    fn unproject(self, point: Vec3) -> Option<Vec3> {
        let world = self.world_from_clip * point.extend(1.0);
        if !world.is_finite() || world.w.abs() < f32::MIN_POSITIVE {
            return None;
        }
        let result = world.truncate() / world.w;
        result.is_finite().then_some(result)
    }

    pub(crate) fn ray(self, normalized: Vec2) -> Option<Ray3d> {
        if !normalized.is_finite() {
            return None;
        }
        let xy = normalized * Vec2::new(2.0, -2.0);
        let near = self.unproject(xy.extend(1.0))?;
        let far = self.unproject(xy.extend(f32::EPSILON))?;
        Some(Ray3d::new(near, Dir3::new(far - near).ok()?)) // 与 Bevy 正式 viewport_to_world 使用同一反向深度端点。
    }

    pub(crate) fn handle_length(self, origin: Vec3) -> Option<f32> {
        let point = self.project(origin)?;
        let clip = self.clip_from_world * origin.extend(1.0);
        let other = self.unproject(Vec3::new(
            point.x * 2.0 + 2.0 / self.size.x / self.inverse_scale,
            -point.y * 2.0,
            clip.z / clip.w,
        ))?;
        let length = other.distance(origin) * HANDLE_LOGICAL_PIXELS;
        (length.is_finite() && length > 0.0).then_some(length)
    }

    /// 在模型根的深度平面构造屏幕朝向方框，缩放手柄不因观察方向或模型倍率变难点击。
    pub(crate) fn screen_square(self, origin: Vec3, half_pixels: f32) -> Option<[Vec3; 4]> {
        let center = self.project(origin)?;
        let clip = self.clip_from_world * origin.extend(1.0);
        let mut corners = [Vec3::ZERO; 4];
        for (corner, offset) in corners.iter_mut().zip([
            Vec2::new(-1.0, -1.0),
            Vec2::new(1.0, -1.0),
            Vec2::new(1.0, 1.0),
            Vec2::new(-1.0, 1.0),
        ]) {
            let point = center + offset * half_pixels / self.size / self.inverse_scale;
            *corner = self.unproject(Vec3::new(point.x * 2.0, -point.y * 2.0, clip.z / clip.w))?;
        }
        Some(corners)
    }

    pub(crate) fn axis_segment(
        self,
        origin: Vec3,
        axis: MoveAxis,
        length: f32,
    ) -> Option<(Vec2, Vec2)> {
        let start = self.project(origin)? * self.size * self.inverse_scale;
        let end =
            self.project(origin + axis.direction() * length)? * self.size * self.inverse_scale;
        ((end - start).length() >= 16.0).then_some((start, end)) // 端视方向投影退化时不显示难以拖动的轴。
    }

    /// 绘制和拾取共用同一四角与退化门；侧着看时不能保留难以点击的细线平面。
    pub(crate) fn plane_quad(self, origin: Vec3, length: f32) -> Option<[Vec2; 4]> {
        let corners = plane_corners(origin, length);
        let mut quad = [Vec2::ZERO; 4];
        for (point, corner) in quad.iter_mut().zip(corners) {
            *point = self.project(corner)? * self.size * self.inverse_scale;
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
        if !normalized.is_finite() {
            return None;
        }
        let length = self.handle_length(origin)?;
        let cursor = normalized * self.size * self.inverse_scale;
        if let Some(quad) = self.plane_quad(origin, length) {
            let mut positive = false;
            let mut negative = false;
            for i in 0..4 {
                let side = (quad[(i + 1) % 4] - quad[i]).perp_dot(cursor - quad[i]);
                positive |= side > 0.001;
                negative |= side < -0.001;
            }
            if !(positive && negative) {
                return Some(MoveHandle::XzPlane);
            } // 方框内部优先于邻轴的宽拾取容差，空白处仍让位给普通选择。
        }
        self.pick_axis(origin, normalized, building)
            .map(MoveHandle::Axis)
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
    let inner = length * PLANE_INNER_FRACTION;
    let outer = length * PLANE_OUTER_FRACTION;
    [
        origin + Vec3::new(inner, 0.0, inner),
        origin + Vec3::new(outer, 0.0, inner),
        origin + Vec3::new(outer, 0.0, outer),
        origin + Vec3::new(inner, 0.0, outer),
    ]
}

/// 平面冻结在按下时的模型Y；使用交点差而非地形射线，防止跳点或读到洞穴的错误表层。
pub(crate) fn handle_point(ray: Ray3d, origin: Vec3, handle: MoveHandle) -> Option<Vec3> {
    match handle {
        MoveHandle::Axis(axis) => {
            axis_scalar(ray, origin, axis).map(|scalar| origin + axis.direction() * scalar)
        }
        MoveHandle::XzPlane | MoveHandle::RotationY => {
            if ray.direction.y.abs() <= 0.0001 {
                return None;
            }
            let distance = (origin.y - ray.origin.y) / ray.direction.y;
            if !distance.is_finite() || distance < 0.0 {
                return None;
            }
            let point = ray.get_point(distance).with_y(origin.y);
            point.is_finite().then_some(point)
        }
        MoveHandle::UniformScale => Some(origin), // 屏幕拖动控制倍率，无须猜测任何地形或世界轴交点。
    }
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
    } // 平面只对X/Z各自吸附，不补偿邻轴或改变Y。
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
