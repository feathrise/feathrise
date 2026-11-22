//! 三轴方帽、平面和等比圆环共用屏幕投影与倍率计算。

use super::{
    constants::{PICK_LOGICAL_PIXELS, ROTATION_SEGMENTS},
    math::{MoveCamera, plane_corners_along, quad_contains},
    types::{MoveAxis, MoveHandle, MovePlane},
};
use bevy::prelude::*;
use feathrise_editor_animation_model::gizmo::constrain_scale_factor;
use feathrise_scene_content::{MAX_SCENE_PLACEMENT_SCALE, MIN_SCENE_PLACEMENT_SCALE};

pub(super) fn frame_scale_basis(degrees: [f32; 3]) -> Mat3 {
    let [x, y, z] = degrees.map(f32::to_radians);
    Mat3::from_quat(Quat::from_euler(EulerRot::XYZ, x, y, z))
}

pub(super) struct ScaleAxisHandle {
    pub tip: Vec3,
    pub cap: [Vec3; 4],
    screen_tip: Vec2,
}

pub(super) struct ScaleGizmo {
    pub axes: [Option<ScaleAxisHandle>; 3],
    pub planes: [Option<[Vec3; 4]>; 3],
    pub outer: Option<[Vec3; ROTATION_SEGMENTS + 1]>,
    screen_center: Vec2,
}

pub(super) const UNIFORM_SCALE_RADIUS: f32 = 8.0;
const SCALE_RING_LOGICAL_PIXELS: f32 = 105.0;

impl ScaleGizmo {
    pub(super) fn new(camera: MoveCamera, origin: Vec3, basis: Mat3, combined: bool) -> Option<Self> {
        let length = camera.handle_length(origin)?;
        let screen_center = camera.logical_point(origin)?;
        // 合并工具保留移动箭头和窄轴末端，缩放方帽及平面放在外侧。
        let (tip_distance, inner, outer) = if combined { (1.32, 0.60, 0.86) } else { (1.0, 0.23, 0.53) };
        let axes = MoveAxis::ALL.map(|axis| {
            let tip = origin + basis * axis.direction() * (length * tip_distance);
            let screen_tip = camera.logical_point(tip)?;
            if screen_center.distance(screen_tip) < 16.0 { return None; }
            Some(ScaleAxisHandle { tip, cap: camera.screen_square(tip, 5.0)?, screen_tip })
        });
        let planes = MovePlane::ALL.map(|plane| {
            let directions = plane.axes().map(|axis| basis * axis.direction());
            let corners = plane_corners_along(origin, directions, length * inner, length * outer);
            camera.project_quad(corners).map(|_| corners)
        });
        let outer = if combined { None } else {
            camera.screen_square(origin, SCALE_RING_LOGICAL_PIXELS).map(|[a, b, _, d]| {
                let horizontal = (b - a) * 0.5;
                let vertical = (d - a) * 0.5;
                std::array::from_fn(|index| {
                    let angle = std::f32::consts::TAU * index as f32 / ROTATION_SEGMENTS as f32;
                    origin + horizontal * angle.cos() + vertical * angle.sin()
                })
            }) // 屏幕同深度的真实世界圆环，透视、正交与 DPI 下均保持固定逻辑尺寸。
        };
        Some(Self { axes, planes, outer, screen_center })
    }

    fn pick(&self, camera: MoveCamera, normalized: Vec2, combined: bool) -> Option<MoveHandle> {
        if !normalized.is_finite() { return None; }
        let cursor = normalized * camera.size * camera.inverse_scale;
        let radius = cursor.distance(self.screen_center);
        if radius <= UNIFORM_SCALE_RADIUS {
            return Some(MoveHandle::UniformScale);
        }
        if let Some(axis) = MoveAxis::ALL.into_iter().filter_map(|axis| {
            let point = self.axes[axis.index()].as_ref()?.screen_tip;
            let distance = (cursor - point).abs().max_element();
            (distance <= PICK_LOGICAL_PIXELS).then_some((axis, distance))
        }).min_by(|(_, a), (_, b)| a.total_cmp(b)).map(|(axis, _)| axis) {
            return Some(MoveHandle::AxisScale(axis));
        }
        for (plane, corners) in MovePlane::ALL.into_iter().zip(self.planes) {
            if corners.and_then(|corners| camera.project_quad(corners)).is_some_and(|quad| quad_contains(quad, cursor)) {
                return Some(MoveHandle::PlaneScale(plane));
            }
        }
        if self.outer.is_some() && (radius - SCALE_RING_LOGICAL_PIXELS).abs() <= PICK_LOGICAL_PIXELS {
            return Some(MoveHandle::UniformScale);
        }
        if combined { return None; } // 复用移动箭头的主轴拾取区域，只点末端方帽才缩放。
        MoveAxis::ALL.into_iter().filter_map(|axis| {
            let handle = self.axes[axis.index()].as_ref()?;
            let line = handle.screen_tip - self.screen_center;
            let t = (cursor - self.screen_center).dot(line) / line.length_squared();
            let distance = cursor.distance(self.screen_center + line * t);
            ((12.0 / line.length()..=1.0).contains(&t) && distance <= PICK_LOGICAL_PIXELS)
                .then_some((MoveHandle::AxisScale(axis), distance))
        }).min_by(|(_, a), (_, b)| a.total_cmp(b)).map(|(handle, _)| handle)
    }
}

pub(super) fn pick_scale_handle(camera: MoveCamera, origin: Vec3, basis: Mat3, cursor: Vec2, combined: bool) -> Option<MoveHandle> {
    ScaleGizmo::new(camera, origin, basis, combined)?.pick(camera, cursor, combined)
}

pub(crate) fn pick_scale(camera: MoveCamera, origin: Vec3, normalized: Vec2) -> bool {
    let Some(center) = camera.project(origin) else {
        return false;
    };
    let offset = (normalized - center) * camera.size * camera.inverse_scale;
    offset.is_finite() && offset.abs().max_element() <= PICK_LOGICAL_PIXELS
}

pub(super) fn scale_values(
    camera: MoveCamera, origin: Vec3, basis: Mat3, start: Vec2, cursor: Vec2,
    handle: MoveHandle, before: [f32; 3], step: f32,
) -> Option<[f32; 3]> {
    let scale = Vec3::from_array(before);
    if !scale.is_finite()
        || scale.min_element() < MIN_SCENE_PLACEMENT_SCALE
        || scale.max_element() > MAX_SCENE_PLACEMENT_SCALE
    {
        return None;
    }
    let axes = MoveAxis::ALL.into_iter().filter(|axis| handle.scales_axis(*axis));
    let bounds = axes.clone().fold((0.0_f32, f32::INFINITY), |(min, max), axis| {
        let value = before[axis.index()];
        (min.max(MIN_SCENE_PLACEMENT_SCALE / value), max.min(MAX_SCENE_PLACEMENT_SCALE / value))
    });
    let factor = scale_drag_factor(camera, origin, basis, start, cursor, handle, step, bounds)?;
    let mut result = before;
    for axis in axes {
        result[axis.index()] = (result[axis.index()] * factor).clamp(MIN_SCENE_PLACEMENT_SCALE, MAX_SCENE_PLACEMENT_SCALE);
    } // 同一平面上的两轴共享倍率，其他轴逐位保留。
    Some(result)
}

pub(super) fn scale_drag_factor(
    camera: MoveCamera, origin: Vec3, basis: Mat3, start: Vec2, cursor: Vec2,
    handle: MoveHandle, step: f32, (min, max): (f32, f32),
) -> Option<f32> {
    let size = camera.size * camera.inverse_scale;
    let center = camera.logical_point(origin)?;
    if handle == MoveHandle::UniformScale {
        let start_radius = (start * size).distance(center);
        let radial_delta = (cursor * size).distance(center) - start_radius;
        // 沿同一半径转向时忽略投影舍入误差，吸附也应保留原比例。
        if radial_delta.abs() <= 1.0e-4 { return constrain_scale_factor(1.0, 0.0, min, max); }
        // 外环按距离比缩放。中心圆用自身半径保底，点中圆心也能从原比例连续放大。
        let factor = 1.0 + radial_delta / start_radius.max(UNIFORM_SCALE_RADIUS);
        return constrain_scale_factor(factor, step, min, max);
    }
    let direction = match handle {
        MoveHandle::AxisScale(axis) => axis.direction(),
        MoveHandle::PlaneScale(plane) => plane.axes().map(MoveAxis::direction).into_iter().sum::<Vec3>(),
        _ => return None,
    };
    let tip = origin + basis * direction * (camera.handle_length(origin)? * 0.36);
    let screen_direction = (camera.logical_point(tip)? - center).try_normalize()?;
    let delta = ((cursor - start) * size).dot(screen_direction);
    group_scale_factor(Vec2::new(delta, 0.0), step, min, max)
}

/// 整组倍率由按下时汇总的共同上下界约束，拖动帧只做常数计算。
pub(crate) use feathrise_editor_animation_model::gizmo::group_scale_factor;
