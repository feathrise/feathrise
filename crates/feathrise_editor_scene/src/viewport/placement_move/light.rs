//! 单个作者灯具的米制光域和锥角。绘制与拾取共享相同的边，未选灯具无需遍历或投影。

use super::{math::MoveCamera, types::{MoveDrag, MoveHandle}};
use crate::{authoring::local_lights, ui::placement_transform::LightField, viewport::tool_preview::foreground::Canvas};
use bevy::prelude::*;
use feathrise_scene_content::ScenePlacementRecord;
use std::{f32::consts::{FRAC_PI_2, TAU}, sync::LazyLock};

const SEGMENTS: usize = super::constants::ROTATION_SEGMENTS;
const CIRCLE: LazyLock<[(f32, f32); SEGMENTS + 1]> = LazyLock::new(|| {
    std::array::from_fn(|i| (TAU * i as f32 / SEGMENTS as f32).sin_cos())
});
const PICK: f32 = super::constants::PICK_LOGICAL_PIXELS;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LightHandle { Range, Inner, Outer }

impl LightHandle {
    pub(crate) const fn field(self) -> LightField {
        match self { Self::Range => LightField::Range, Self::Inner => LightField::InnerAngle, Self::Outer => LightField::OuterAngle }
    }

    pub(crate) fn value(self, record: &ScenePlacementRecord) -> Option<f32> { self.field().value(record) }
    pub(super) fn label(self) -> &'static str { self.field().label() }

    fn color(self) -> Color {
        match self {
            Self::Range => Color::srgb_u8(255, 193, 82),
            Self::Outer => Color::srgb_u8(246, 146, 53),
            Self::Inner => Color::srgb_u8(83, 222, 231),
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct LightGuide {
    origin: Vec3,
    basis: Mat3,
    range: f32,
    cone: Option<[f32; 2]>,
}

fn ring(center: Vec3, a: Vec3, b: Vec3, mut edge: impl FnMut(Vec3, Vec3)) {
    let mut previous = center + a;
    for &(sin, cos) in &CIRCLE[1..] {
        let next = center + cos * a + sin * b;
        edge(previous, next);
        previous = next;
    }
}

fn closest(point: Vec2, a: Vec2, b: Vec2) -> (f32, f32) {
    let delta = b - a;
    let t = if delta.length_squared() > f32::EPSILON {
        ((point - a).dot(delta) / delta.length_squared()).clamp(0.0, 1.0)
    } else { 0.0 };
    (point.distance_squared(a + t * delta), t)
}

impl LightGuide {
    pub(super) fn new(origin: Vec3, basis: Mat3, record: &ScenePlacementRecord) -> Option<Self> {
        let (_, _, range, cone) = local_lights::parameters(&record.asset_target)?;
        Some(Self { origin, basis, range: range.unwrap_or(20.0), cone })
    }

    fn edges(self, mut emit: impl FnMut(LightHandle, Vec3, Vec3)) {
        let Some([inner, outer]) = self.cone else {
            for (a, b) in [(Vec3::X, Vec3::Y), (Vec3::Y, Vec3::Z), (Vec3::Z, Vec3::X)] {
                ring(self.origin, a * self.range, b * self.range, |a, b| emit(LightHandle::Range, a, b));
            }
            return;
        };
        let front = -self.basis.z_axis;
        let sides = [self.basis.x_axis, self.basis.y_axis, -self.basis.x_axis, -self.basis.y_axis];
        // KHR_lights_punctual 的范围是到光源的球面距离。外圈位于球面，内圈缩短以分开 0° 内锥和轴端范围手柄。
        for (handle, angle, distance) in [(LightHandle::Outer, outer, self.range), (LightHandle::Inner, inner, self.range * 0.68)] {
            let (sin, cos) = angle.sin_cos();
            let center = self.origin + front * (distance * cos);
            let radius = distance * sin;
            ring(center, self.basis.x_axis * radius, self.basis.y_axis * radius, |a, b| emit(handle, a, b));
            for side in sides { emit(handle, self.origin, center + side * radius); }
        }
        // 范围前端的四条球面弧在正视光轴时仍可抓取，便于从任一视角单独调整射程。
        for side in sides {
            let mut previous = self.origin + front * self.range;
            for i in 1..=SEGMENTS / 4 {
                let (sin, cos) = (outer * i as f32 / (SEGMENTS / 4) as f32).sin_cos();
                let next = self.origin + (front * cos + side * sin) * self.range;
                emit(LightHandle::Range, previous, next);
                previous = next;
            }
        }
    }

    pub(super) fn draw(self, camera: MoveCamera, canvas: &mut Canvas, active: Option<MoveHandle>) {
        self.edges(|handle, a, b| {
            let selected = active == Some(MoveHandle::Light(handle));
            let color = if selected { Color::WHITE } else { handle.color() };
            canvas.world_line(camera, a, b, if selected { 2.5 } else { 1.5 }, color);
        });
        if self.cone.is_some() && let Some(p) = canvas.point(camera, self.origin - self.basis.z_axis * self.range) {
            let d = 4.0;
            canvas.quad([p - Vec2::Y*d, p + Vec2::X*d, p + Vec2::Y*d, p - Vec2::X*d], LightHandle::Range.color());
        }
    }

    pub(super) fn pick(self, camera: MoveCamera, cursor: Vec2) -> Option<(LightHandle, Vec3)> {
        let size = camera.size * camera.inverse_scale;
        let mut nearest = PICK * PICK;
        let mut result = None;
        self.edges(|handle, a, b| {
            let Some((pa, pb)) = camera.project(a).zip(camera.project(b)) else { return; };
            let (distance, t) = closest(cursor * size, pa * size, pb * size);
            if distance < nearest {
                nearest = distance;
                result = Some((handle, a.lerp(b, t)));
            }
        });
        result
    }
}

// 用冻结的世界径向和相机射线求最近点。光轴正对相机时改用按下深度的屏幕平面，向上拖会增大射程。
fn range_delta(camera: MoveCamera, origin: Vec3, grip: Vec3, start: Vec2, cursor: Vec2) -> Option<f32> {
    let axis = (grip - origin).try_normalize()?;
    let first = camera.ray(start)?;
    let last = camera.ray(cursor)?;
    let scalar = |ray: Ray3d| {
        let dot = axis.dot(*ray.direction);
        let perpendicular = 1.0 - dot * dot;
        let offset = ray.origin - origin;
        (perpendicular > 0.0001).then(|| (axis.dot(offset) - dot * ray.direction.dot(offset)) / perpendicular)
    };
    if let Some((first, last)) = scalar(first).zip(scalar(last)) { return Some(last - first); }
    let normal = *first.direction;
    let point = |ray: Ray3d| ray.get_point((grip - ray.origin).dot(normal) / ray.direction.dot(normal));
    let delta = cursor - start;
    let sign = if delta.y.abs() >= delta.x.abs() { -delta.y.signum() } else { delta.x.signum() };
    Some(sign * point(first).distance(point(last)))
}

pub(super) fn update(drag: &mut MoveDrag, handle: LightHandle, cursor: Vec2, moved: bool) -> Result<(), String> {
    if !moved {
        drag.preview.asset_target = drag.before.asset_target.clone();
        return Ok(());
    }
    let (rgb, cd, mut range, mut cone) = local_lights::parameters(&drag.before.asset_target).ok_or("灯具身份已经变化")?;
    let grip = drag.start_point;
    match handle {
        LightHandle::Range => {
            let delta = range_delta(drag.camera, drag.world_origin, grip, drag.start_cursor, cursor).ok_or("当前视角无法计算照射范围")?;
            if !delta.is_finite() { return Err("照射范围无效".into()); }
            range = Some((range.unwrap_or(20.0) + delta).max(f32::MIN_POSITIVE));
        }
        LightHandle::Inner | LightHandle::Outer => {
            let angles = cone.as_mut().ok_or("当前灯具缺少聚光锥")?;
            let basis = super::input::placement_scale_basis(drag.frame, &drag.before);
            let forward = -basis.z_axis;
            let offset = grip - drag.world_origin;
            let angular = offset - forward * offset.dot(forward);
            let index = usize::from(handle == LightHandle::Outer);
            // 默认内锥为零，轴上起点没有方位。把拖动方向投回灯具截面，自任一侧均可展开。
            let side = if angles[index] > 0.0 && let Some(side) = angular.try_normalize() { side } else {
                let project = |p| drag.camera.project(p).ok_or("锥角在相机后方");
                let center = project(grip)?;
                let x = project(grip + basis.x_axis)? - center;
                let y = project(grip + basis.y_axis)? - center;
                let motion = cursor - drag.start_cursor;
                let determinant = x.perp_dot(y);
                let weights = if determinant.abs() > x.length() * y.length() * 0.001 {
                    Vec2::new(motion.perp_dot(y), x.perp_dot(motion)) / determinant
                } else { Vec2::new(motion.dot(x), motion.dot(y)) };
                (basis.x_axis * weights.x + basis.y_axis * weights.y).try_normalize().unwrap_or(basis.x_axis)
            };
            let (lo, hi) = if index == 0 { (0.0, angles[1].next_down()) } else { (angles[0].next_up(), FRAC_PI_2) };
            // 沿按下方位的投影四分之一圆寻找光标最近位置，透视与正交共用，端视时也不依赖面交点。
            let radius = offset.length();
            let point = |angle: f32| {
                let (sin, cos) = angle.sin_cos();
                drag.camera.project(drag.world_origin + (forward*cos + side*sin) * radius)
            };
            let size = drag.camera.size * drag.camera.inverse_scale;
            let goal = (point(angles[index]).ok_or("锥角在相机后方")? + cursor - drag.start_cursor) * size;
            let mut previous = (lo, point(lo));
            let mut nearest = None;
            for i in 1..=SEGMENTS {
                let angle = lo + (hi - lo) * (i as f32 / SEGMENTS as f32);
                let projected = point(angle);
                if let Some((a, b)) = previous.1.zip(projected) {
                    let (d, t) = closest(goal, a * size, b * size);
                    if nearest.is_none_or(|(best, _)| d < best) { nearest = Some((d, previous.0 + t * (angle - previous.0))); }
                }
                previous = (angle, projected);
            }
            angles[index] = nearest.ok_or("当前视角无法投影锥角")?.1.clamp(lo, hi);
        }
    }
    drag.preview.asset_target = local_lights::target((rgb, cd, range, cone))?;
    Ok(())
}

#[cfg(any(test, feature = "validation"))]
#[path = "light_validation.rs"]
pub(super) mod validation;
