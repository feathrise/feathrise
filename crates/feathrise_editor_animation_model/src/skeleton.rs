//! GLB 动画与蒙皮工具共用的骨架标记、轮廓和配色。

use bevy::prelude::*;

use super::gizmo::GizmoCamera;

pub const JOINT_COLOR: Color = Color::srgb(0.96, 0.98, 1.0);
pub const BONE_COLOR: Color = Color::srgba(0.87, 0.93, 0.99, 0.85);
pub const SELECTED_COLOR: Color = Color::srgb(0.15, 1.0, 1.0);
pub const LINE_WIDTH: f32 = 1.5;
pub const SELECTED_LINE_WIDTH: f32 = 3.5;
pub const DEPTH_BIAS: f32 = -1.0; // 骨架轮廓透过角色表面，在预览相机中仍可见和拾取。
pub const PICK_PADDING_PX: f32 = 3.0;

const JOINT_RADIUS_PX: f32 = 3.5;
const SELECTED_JOINT_RADIUS_PX: f32 = 5.0;
const MIN_JOINT_RADIUS_PX: f32 = 1.5;

#[derive(Clone, Copy)]
pub struct JointGlyph {
    pub origin: Vec3,
    /// 相对画布中心的逻辑像素，与骨骼拾取使用的坐标一致。
    pub screen: Vec2,
    pub radius: f32,
    right: Vec3,
    down: Vec3,
}

impl JointGlyph {
    pub fn project(camera: GizmoCamera, origin: Vec3, selected: bool) -> Option<Self> {
        let screen = camera.project(origin)? * camera.size * camera.inverse_scale;
        let [top_left, top_right, _, bottom_left] = camera.screen_square(origin, 1.0)?;
        // 每个关节保留自身深度处的一像素基向量，透视下远近端仍按屏幕像素留出间隔。
        Some(Self {
            origin,
            screen,
            right: (top_right - top_left) * 0.5,
            down: (bottom_left - top_left) * 0.5,
            radius: if selected {
                SELECTED_JOINT_RADIUS_PX
            } else {
                JOINT_RADIUS_PX
            },
        })
    }

    pub fn offset(self, pixels: Vec2) -> Vec3 {
        self.origin + self.right * pixels.x + self.down * pixels.y
    }

    pub fn diamond(self) -> [Vec3; 5] {
        let right = self.right * self.radius;
        let down = self.down * self.radius;
        let top = self.origin - down;
        [
            top,
            self.origin + right,
            self.origin + down,
            self.origin - right,
            top,
        ]
    }

    fn bone_offsets(self, tail: Self) -> Option<[Vec2; 3]> {
        let axis = tail.screen - self.screen;
        let length = axis.length();
        let head_gap = self.radius + 2.0;
        let tail_gap = tail.radius + 2.0;
        let space = length - head_gap - tail_gap;
        if !space.is_finite() || space < 3.0 {
            // 端视重合或手指过短时保留各自的起点，避免骨段轮廓盖住两个关节。
            return None;
        }
        let axis = axis / length;
        // 宽端紧接骨骼起点，尖端指向子关节。两端留白让共线骨段也有清楚的边界。
        let width = (length * 0.085).clamp(1.5, 5.0).min(space * 0.5);
        let side = Vec2::new(-axis.y, axis.x) * width;
        let base = axis * head_gap;
        Some([base + side, base - side, -axis * tail_gap])
    }

    pub fn bone_to(self, tail: Self) -> Option<[Vec3; 4]> {
        let [left, right, tip] = self.bone_offsets(tail)?;
        let left = self.offset(left);
        Some([left, self.offset(right), tail.offset(tip), left])
    }

    pub fn screen_bone_to(self, tail: Self) -> Option<[Vec2; 3]> {
        let [left, right, tip] = self.bone_offsets(tail)?;
        Some([self.screen + left, self.screen + right, tail.screen + tip])
    }
}

pub fn fit_connected_joints(
    glyphs: &mut [Option<JointGlyph>],
    parent: usize,
    child: usize,
    selected: Option<usize>,
) {
    let Some((head, tail)) = glyphs
        .get(parent)
        .copied()
        .flatten()
        .zip(glyphs.get(child).copied().flatten())
    else {
        return;
    };
    // 手指等密集关节按相邻起点的间距收小，放大后恢复尺寸，选中标记始终醒目。
    let radius = (head.screen.distance(tail.screen) * 0.4).max(MIN_JOINT_RADIUS_PX);
    for endpoint in [parent, child] {
        if Some(endpoint) != selected
            && let Some(glyph) = &mut glyphs[endpoint]
        {
            glyph.radius = glyph.radius.min(radius);
        }
    }
}

pub fn triangle_distance_squared(cursor: Vec2, [a, b, c]: [Vec2; 3]) -> f32 {
    let mut positive = false;
    let mut negative = false;
    let mut closest = f32::INFINITY;
    for (start, end) in [(a, b), (b, c), (c, a)] {
        let edge = end - start;
        let relative = cursor - start;
        let side = edge.perp_dot(relative);
        positive |= side > 0.0;
        negative |= side < 0.0;
        let t = (relative.dot(edge) / edge.length_squared()).clamp(0.0, 1.0);
        closest = closest.min((relative - edge * t).length_squared());
    }
    if positive && negative { closest } else { 0.0 }
}
