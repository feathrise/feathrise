//! 由资产动画与场景网格共用的固定预算手柄和冻结视角。

use bevy::prelude::*;
use feathrise_editor_core::view_projection::ViewProjection;

const HANDLE_LOGICAL_PIXELS: f32 = 80.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveAxis {
    X,
    Y,
    Z,
}

impl MoveAxis {
    pub const ALL: [Self; 3] = [Self::X, Self::Y, Self::Z];
    pub fn index(self) -> usize {
        match self {
            Self::X => 0,
            Self::Y => 1,
            Self::Z => 2,
        }
    }
    pub fn direction(self) -> Vec3 {
        match self {
            Self::X => Vec3::X,
            Self::Y => Vec3::Y,
            Self::Z => Vec3::Z,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::X => "X",
            Self::Y => "Y",
            Self::Z => "Z",
        }
    }
    pub fn color(self) -> Color {
        match self {
            Self::X => Color::srgb(0.91, 0.22, 0.31),
            Self::Y => Color::srgb(0.46, 0.72, 0.16),
            Self::Z => Color::srgb(0.20, 0.48, 0.86),
        } // 与视口方向轴采用同一三色约定，悬停和拖动再以铜色强调。
    }
}

/// 选择元素类型与操作工具彼此独立；切工具不能退出网格编辑或改为变换完整对象。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MeshEditTool {
    #[default]
    Select,
    Move,
    Rotate,
    Scale,
}

impl MeshEditTool {
    pub fn label(self) -> &'static str {
        match self {
            Self::Select => "选择",
            Self::Move => "移动",
            Self::Rotate => "旋转",
            Self::Scale => "缩放",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveConstraint {
    View,
    X,
    Y,
    Z,
}

impl MoveConstraint {
    pub fn axis(self) -> Option<Vec3> {
        match self {
            Self::View => None,
            Self::X => Some(Vec3::X),
            Self::Y => Some(Vec3::Y),
            Self::Z => Some(Vec3::Z),
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::View => "视图平面",
            Self::X => "世界 X",
            Self::Y => "世界 Y",
            Self::Z => "世界 Z",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GizmoCamera(ViewProjection);

impl std::ops::Deref for GizmoCamera {
    type Target = ViewProjection;

    fn deref(&self) -> &ViewProjection {
        &self.0
    }
}

impl GizmoCamera {
    pub fn from_projection(
        projection: &Projection,
        transform: &GlobalTransform,
        size: Vec2,
        inverse_scale: f32,
    ) -> Option<Self> {
        ViewProjection::from_projection(projection, transform, size, inverse_scale).map(Self)
    }

    pub fn from_view(
        projection: Projection,
        view: Mat4,
        size: Vec2,
        inverse_scale: f32,
    ) -> Option<Self> {
        ViewProjection::from_view(projection, view, size, inverse_scale).map(Self)
    }

    pub fn projection(self) -> ViewProjection {
        self.0
    }

    pub fn handle_length(self, origin: Vec3) -> Option<f32> {
        let point = self.project(origin)?;
        let clip = self.clip(origin);
        let other = self.unproject(Vec3::new(
            point.x * 2.0 + 2.0 / self.size.x / self.inverse_scale,
            -point.y * 2.0,
            clip.z / clip.w,
        ))?;
        let length = other.distance(origin) * HANDLE_LOGICAL_PIXELS;
        (length.is_finite() && length > 0.0).then_some(length)
    }

    /// 在模型根的深度平面构造屏幕朝向方框，缩放手柄不因观察方向或模型倍率变难点击。
    pub fn screen_square(self, origin: Vec3, half_pixels: f32) -> Option<[Vec3; 4]> {
        let center = self.project(origin)?;
        let clip = self.clip(origin);
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

    pub fn axis_segment(
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

}

pub const RING_SEGMENTS: usize = 64; // 四个圆环最多256段，不跟随场景规模或模型面数增长。
const PICK_PIXELS: f32 = 8.0;

pub struct MoveArrow {
    pub constraint: MoveConstraint,
    pub world: [Vec3; 4], // 起点、轴末端及两条箭翼端点；world和screen同时通过退化门。
    pub screen: [Vec2; 4],
}

pub struct RotationRing {
    pub constraint: MoveConstraint,
    pub world: [Vec3; RING_SEGMENTS + 1],
    pub screen: [Vec2; RING_SEGMENTS + 1],
    pub edge_on: bool,
}

pub struct ScaleHandle {
    pub constraint: MoveConstraint,
    pub world: [Vec3; 6],
    pub screen: [Vec2; 6],
}

/// 三轴末端使用方帽，与移动箭头区分；中心方块控制整体倍率，绘制与拾取共享投影。
pub fn scale_handles(camera: GizmoCamera, origin: Vec3) -> [Option<ScaleHandle>; 3] {
    let length = camera.handle_length(origin);
    MoveAxis::ALL.map(|axis| {
        let length = length?;
        let (start, end) = camera.axis_segment(origin, axis, length)?;
        let tip = origin + axis.direction() * length;
        let corners = camera.screen_square(tip, 4.0)?;
        let world = [origin, tip, corners[0], corners[1], corners[2], corners[3]];
        let mut screen = [start, end, Vec2::ZERO, Vec2::ZERO, Vec2::ZERO, Vec2::ZERO];
        for i in 2..6 {
            screen[i] = camera.project(world[i])? * camera.size * camera.inverse_scale;
        }
        Some(ScaleHandle {
            constraint: constraint(axis),
            world,
            screen,
        })
    })
}

/// 沿用对象缩放的指数拖动手感；单轴只取屏幕上的轴向分量，起点在中心也能立即操作。
pub fn scale_factor(
    camera: GizmoCamera,
    start: Vec2,
    current: Vec2,
    origin: Vec3,
    constraint: MoveConstraint,
    snapping: feathrise_editor_core::EditorSnapSettings,
    control: bool,
) -> Result<f32, String> {
    let delta = (current - start) * camera.size * camera.inverse_scale;
    let delta = if constraint == MoveConstraint::View {
        delta
    } else {
        let axis = scale_handles(camera, origin)
            .into_iter()
            .flatten()
            .find(|handle| handle.constraint == constraint)
            .ok_or("当前视角与缩放轴平行，请调整视角")?;
        Vec2::new(
            delta.dot((axis.screen[1] - axis.screen[0]).normalize()),
            0.0,
        )
    };
    group_scale_factor(
        delta,
        if snapping.active(control) {
            snapping.scale_factor
        } else {
            0.0
        },
        0.0001,
        10000.0,
    )
    .ok_or_else(|| "缩放倍率不是有限正数".into())
}

fn constraint(axis: MoveAxis) -> MoveConstraint {
    match axis {
        MoveAxis::X => MoveConstraint::X,
        MoveAxis::Y => MoveConstraint::Y,
        MoveAxis::Z => MoveConstraint::Z,
    }
}

/// 沿用完整对象手柄的80逻辑像素和端视轴拒绝门，但元素中心使用视图平面而非XZ平面。
pub fn move_arrows(camera: GizmoCamera, origin: Vec3) -> [Option<MoveArrow>; 3] {
    let length = camera.handle_length(origin);
    MoveAxis::ALL.map(|axis| {
        let length = length?;
        let (start, end) = camera.axis_segment(origin, axis, length)?;
        let direction = axis.direction();
        let tip = origin + direction * length;
        let view = *camera.ray(camera.project(tip)?)?.direction;
        let side = direction.cross(view).try_normalize()?;
        let world = [
            origin,
            tip,
            tip - direction * length * 0.16 + side * length * 0.07,
            tip - direction * length * 0.16 - side * length * 0.07,
        ];
        let logical = camera.size * camera.inverse_scale;
        let mut screen = [start, end, Vec2::ZERO, Vec2::ZERO];
        screen[2] = camera.project(world[2])? * logical;
        screen[3] = camera.project(world[3])? * logical;
        Some(MoveArrow {
            constraint: constraint(axis),
            world,
            screen,
        })
    })
}

fn rotation_axis(camera: GizmoCamera, constraint: MoveConstraint) -> Option<Vec3> {
    constraint
        .axis()
        .or_else(|| camera.ray(Vec2::ZERO).map(|ray| *ray.direction))
}

/// XYZ环使用相同半径，视图环稍大；侧视收成细线时仍绘制和拾取，拖动改用屏幕切向。
pub fn rotation_rings(camera: GizmoCamera, origin: Vec3) -> [Option<RotationRing>; 4] {
    let radius = camera.handle_length(origin).map(|length| length * 0.9);
    [
        MoveConstraint::X,
        MoveConstraint::Y,
        MoveConstraint::Z,
        MoveConstraint::View,
    ]
    .map(|constraint| {
        let radius = radius?
            * if constraint == MoveConstraint::View {
                1.2
            } else {
                1.0
            };
        let axis = rotation_axis(camera, constraint)?;
        let seed = if axis.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
        let u = (seed - axis * axis.dot(seed)).try_normalize()?;
        let v = axis.cross(u).try_normalize()?;
        let logical = camera.size * camera.inverse_scale;
        let mut world = [Vec3::ZERO; RING_SEGMENTS + 1];
        let mut screen = [Vec2::ZERO; RING_SEGMENTS + 1];
        let mut min = Vec2::splat(f32::INFINITY);
        let mut max = Vec2::splat(f32::NEG_INFINITY);
        for index in 0..=RING_SEGMENTS {
            let angle = index as f32 / RING_SEGMENTS as f32 * std::f32::consts::TAU;
            world[index] = origin + (u * angle.cos() + v * angle.sin()) * radius;
            screen[index] = camera.project(world[index])? * logical;
            min = min.min(screen[index]);
            max = max.max(screen[index]);
        }
        // 面积/最长轴识别侧视，包括斜对角细线；只切换拖动算法，不能让整条轴突然消失。
        let center = camera.project(origin)? * logical;
        let twice_area: f32 = screen
            .windows(2)
            .map(|segment| (segment[0] - center).perp_dot(segment[1] - center))
            .sum();
        let major = (max - min).max_element();
        if major < 16.0 {
            return None;
        }
        Some(RotationRing {
            constraint,
            world,
            screen,
            edge_on: twice_area.abs() / major < 12.0,
        })
    })
}

fn segment_distance(point: Vec2, a: Vec2, b: Vec2) -> f32 {
    let edge = b - a;
    if edge.length_squared() < 1e-6 {
        return f32::INFINITY;
    }
    point.distance(a + edge * ((point - a).dot(edge) / edge.length_squared()).clamp(0.0, 1.0))
}

pub fn pick_transform_handle(
    tool: MeshEditTool,
    camera: GizmoCamera,
    origin: Vec3,
    cursor: Vec2,
) -> Option<MoveConstraint> {
    if !cursor.is_finite() {
        return None;
    }
    let logical = camera.size * camera.inverse_scale;
    let point = cursor * logical;
    let mut nearest = None;
    match tool {
        MeshEditTool::Select => None,
        MeshEditTool::Move => {
            let center = camera.project(origin)? * logical;
            if (point - center).abs().max_element() <= 7.0 {
                return Some(MoveConstraint::View);
            }
            for arrow in move_arrows(camera, origin).into_iter().flatten() {
                let [a, b, c, d] = arrow.screen;
                let start = a.lerp(b, (12.0 / a.distance(b)).min(1.0));
                let distance = segment_distance(point, start, b)
                    .min(segment_distance(point, b, c))
                    .min(segment_distance(point, b, d));
                if distance <= PICK_PIXELS && nearest.is_none_or(|(_, best)| distance < best) {
                    nearest = Some((arrow.constraint, distance));
                }
            }
            nearest.map(|(constraint, _)| constraint)
        }
        MeshEditTool::Rotate => {
            // 侧视圆环穿过轴心时给骨节/中心选择留空，避免细线抢走中心点击。
            if point.distance(camera.project(origin)? * logical) < 12.0 {
                return None;
            }
            for ring in rotation_rings(camera, origin).into_iter().flatten() {
                let distance = ring
                    .screen
                    .windows(2)
                    .map(|segment| segment_distance(point, segment[0], segment[1]))
                    .fold(f32::INFINITY, f32::min);
                if distance <= PICK_PIXELS && nearest.is_none_or(|(_, best)| distance < best) {
                    nearest = Some((ring.constraint, distance));
                }
            }
            nearest.map(|(constraint, _)| constraint)
        }
        MeshEditTool::Scale => {
            let center = camera.project(origin)? * logical;
            if (point - center).abs().max_element() <= 8.0 {
                return Some(MoveConstraint::View);
            }
            for handle in scale_handles(camera, origin).into_iter().flatten() {
                let [a, b, c, d, e, f] = handle.screen;
                let start = a.lerp(b, (12.0 / a.distance(b)).min(1.0));
                let distance = segment_distance(point, start, b)
                    .min(segment_distance(point, c, d))
                    .min(segment_distance(point, d, e))
                    .min(segment_distance(point, e, f))
                    .min(segment_distance(point, f, c));
                if distance <= PICK_PIXELS && nearest.is_none_or(|(_, best)| distance < best) {
                    nearest = Some((handle.constraint, distance));
                }
            }
            nearest.map(|(constraint, _)| constraint)
        }
    }
}

/// 正常视角在冻结的世界平面求有向角；侧视改用屏幕切向，避免求交退化。
pub fn rotation_delta(
    camera: GizmoCamera,
    start: Vec2,
    current: Vec2,
    origin: Vec3,
    constraint: MoveConstraint,
) -> Result<(Vec3, f32), String> {
    let ring = rotation_rings(camera, origin)
        .into_iter()
        .flatten()
        .find(|ring| ring.constraint == constraint)
        .ok_or("旋转环越过裁剪面，请调整视角后重试")?;
    let axis = rotation_axis(camera, constraint).ok_or("无法计算旋转轴")?;
    if !start.is_finite() || !current.is_finite() {
        return Err("旋转指针不在有效视口内".into());
    }
    let center = camera.project(origin).ok_or("旋转中心不在有效视口内")?;
    let logical = camera.size * camera.inverse_scale;
    if ((start - center) * logical).length() < 8.0 {
        return Err("指针过于接近旋转轴心".into());
    }
    if ring.edge_on {
        // 射线与旋转平面近平行时求交会失效或跳角。冻结相机前侧的切向，
        // 按72逻辑像素一弧度连续拖动；即使环完全重合为直线，也能操作该轴。
        let view = camera.ray(center).ok_or("无法计算旋转视线")?;
        let tangent = axis
            .cross(-*view.direction)
            .try_normalize()
            .ok_or("无法计算旋转切向")?;
        let radius = camera.handle_length(origin).ok_or("无法计算旋转半径")? * 0.9;
        let tip = camera
            .project(origin + tangent * radius)
            .ok_or("旋转切向越过裁剪面")?;
        let direction = ((tip - center) * logical)
            .try_normalize()
            .ok_or("无法计算旋转切向")?;
        let radians = ((current - start) * logical).dot(direction) / 72.0;
        return if radians.is_finite() {
            Ok((axis, radians))
        } else {
            Err("旋转角度不是有限数值".into())
        };
    }
    let point = |cursor| -> Result<Vec3, String> {
        let ray = camera.ray(cursor).ok_or("旋转指针不在有效视口内")?;
        let denominator = ray.direction.dot(axis);
        if denominator.abs() < 0.02 {
            return Err("旋转平面接近端视，请调整视角后重试".into());
        }
        let distance = (origin - ray.origin).dot(axis) / denominator;
        if !distance.is_finite() || distance < 0.0 {
            return Err("旋转平面位于相机后方".into());
        }
        (ray.get_point(distance) - origin)
            .try_normalize()
            .ok_or_else(|| "请从旋转环开始拖动，避免旋转轴心".into())
    };
    if ((current - center) * logical).length() < 8.0 {
        return Err("指针过于接近旋转轴心".into());
    }
    let a = point(start)?;
    let b = point(current)?;
    let radians = axis.dot(a.cross(b)).atan2(a.dot(b));
    if !radians.is_finite() {
        return Err("旋转角度不是有限数值".into());
    }
    Ok((axis, radians))
}

pub const SCALE_DOUBLE_LOGICAL_PIXELS: f32 = 120.0; // 拖动120逻辑像素翻倍，指数变化兼顾小模型精调。

pub fn group_scale_factor(logical_delta: Vec2, step: f32, minimum: f32, maximum: f32) -> Option<f32> {
    if !logical_delta.is_finite() || !minimum.is_finite() || !maximum.is_finite()
        || minimum <= 0.0 || maximum < minimum { return None; }
    let exponent = ((logical_delta.x - logical_delta.y) / SCALE_DOUBLE_LOGICAL_PIXELS).clamp(-30.0, 30.0);
    let mut factor = 2.0_f32.powf(exponent);
    if step.is_finite() && step > 0.0 { factor = (factor / step).round() * step; }
    Some(factor.clamp(minimum, maximum))
}
