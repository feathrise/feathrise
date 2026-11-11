//! 世界Y旋转圆环的实际投影、固定线段拾取与相对角度；不读取Mesh或场景层级。

use super::{constants::*, math::MoveCamera, types::RotationRing};
use bevy::prelude::*;

pub(crate) fn rotation_ring(camera: MoveCamera, origin: Vec3) -> Option<RotationRing> {
    let radius = camera.handle_length(origin)? * ROTATION_RADIUS_FRACTION;
    let mut ring = RotationRing {
        world: [Vec3::ZERO; ROTATION_SEGMENTS + 1],
        screen: [Vec2::ZERO; ROTATION_SEGMENTS + 1],
    };
    let mut min = Vec2::splat(f32::INFINITY);
    let mut max = Vec2::splat(f32::NEG_INFINITY);
    for i in 0..=ROTATION_SEGMENTS {
        let angle = std::f32::consts::TAU * i as f32 / ROTATION_SEGMENTS as f32;
        ring.world[i] = origin + yaw_direction(angle.to_degrees()) * radius;
        ring.screen[i] = camera.project(ring.world[i])? * camera.size * camera.inverse_scale;
        min = min.min(ring.screen[i]);
        max = max.max(ring.screen[i]);
    }
    ((max - min).min_element() >= ROTATION_MIN_THICKNESS).then_some(ring)
}

pub(crate) fn pick_rotation(camera: MoveCamera, origin: Vec3, normalized: Vec2) -> bool {
    let Some(ring) = rotation_ring(camera, origin) else {
        return false;
    };
    let cursor = normalized * camera.size * camera.inverse_scale;
    let Some(center) = camera.project(origin) else {
        return false;
    };
    if !cursor.is_finite() || cursor.distance(center * camera.size * camera.inverse_scale) <= 12.0 {
        return false;
    }
    ring.screen.windows(2).any(|edge| {
        let segment = edge[1] - edge[0];
        let length_squared = segment.length_squared();
        if length_squared <= f32::EPSILON {
            return false;
        }
        let t = ((cursor - edge[0]).dot(segment) / length_squared).clamp(0.0, 1.0);
        cursor.distance(edge[0] + segment * t) <= PICK_LOGICAL_PIXELS
    })
}

pub(crate) fn yaw_direction(degrees: f32) -> Vec3 {
    let angle = degrees.to_radians();
    Vec3::new(angle.cos(), 0.0, -angle.sin()) // 与Quat::from_rotation_y同向，不能用XZ的常规atan2把拖动方向反过来。
}

pub(crate) fn rotation_yaw(
    before: f32,
    origin: Vec3,
    grabbed: Vec3,
    current: Vec3,
    step_degrees: f32,
) -> Option<f32> {
    let start = (grabbed - origin).xz();
    let end = (current - origin).xz();
    if !before.is_finite()
        || !start.is_finite()
        || !end.is_finite()
        || start.length_squared() <= f32::EPSILON
        || end.length_squared() <= start.length_squared() * 0.0025
    {
        return None;
    } // 接近轴心时方向不确定，保留上次预览但不准松手采用。
    let delta = (-end.y).atan2(end.x) - (-start.y).atan2(start.x);
    let mut yaw = before + delta.to_degrees();
    if step_degrees.is_finite() && step_degrees > 0.0 {
        yaw = (yaw / step_degrees).round() * step_degrees;
    }
    yaw.is_finite().then(|| yaw.rem_euclid(360.0)) // 整圈等价；跨±180°不翻转，也不需要积累任务或历史。
}
