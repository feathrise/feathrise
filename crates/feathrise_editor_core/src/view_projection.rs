//! 视口投影快照。手柄和 GLB 后台拾取共用同一帧的投影数学。

use bevy::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewProjection {
    clip_from_world: Mat4,
    world_from_clip: Mat4,
    pub size: Vec2,
    pub inverse_scale: f32,
}

impl ViewProjection {
    /// 资产预览复用同一投影数学，只传入其既有相机，不新增场景相机状态。
    pub fn from_projection(
        projection: &Projection,
        transform: &GlobalTransform,
        size: Vec2,
        inverse_scale: f32,
    ) -> Option<Self> {
        Self::from_view(
            projection.clone(),
            transform.to_matrix(),
            size,
            inverse_scale,
        )
    }

    pub fn from_view(
        mut projection: Projection,
        world_from_view: Mat4,
        size: Vec2,
        inverse_scale: f32,
    ) -> Option<Self> {
        if !size.is_finite() || size.min_element() <= 0.0 {
            return None;
        }
        projection.update(size.x, size.y);
        let clip_from_world = projection.get_clip_from_view() * world_from_view.inverse();
        let world_from_clip = clip_from_world.inverse();
        (clip_from_world.is_finite() && world_from_clip.is_finite()).then_some(Self {
            clip_from_world,
            world_from_clip,
            size,
            inverse_scale,
        })
    }

    pub fn project(self, world: Vec3) -> Option<Vec2> {
        let clip = self.clip(world);
        if !clip.is_finite() || clip.w <= 0.0 {
            return None;
        }
        let ndc = clip.truncate() / clip.w;
        if !(0.0..=1.0).contains(&ndc.z) {
            return None;
        } // 不显示近裁剪后方或正交深度外的手柄。
        Some(Vec2::new(ndc.x, -ndc.y) * 0.5)
    }

    pub fn unproject(self, point: Vec3) -> Option<Vec3> {
        let world = self.world_from_clip * point.extend(1.0);
        if !world.is_finite() || world.w.abs() < f32::MIN_POSITIVE {
            return None;
        }
        let result = world.truncate() / world.w;
        result.is_finite().then_some(result)
    }

    pub fn ray(self, normalized: Vec2) -> Option<Ray3d> {
        if !normalized.is_finite() {
            return None;
        }
        let xy = normalized * Vec2::new(2.0, -2.0);
        let near = self.unproject(xy.extend(1.0))?;
        let far = self.unproject(xy.extend(f32::EPSILON))?;
        Some(Ray3d::new(near, Dir3::new(far - near).ok()?)) // 与 Bevy 正式 viewport_to_world 使用同一反向深度端点。
    }

    pub fn clip(self, world: Vec3) -> Vec4 {
        self.clip_from_world * world.extend(1.0)
    }
}
