//! 场景区域工具共用视口像素换算与水平面取点，交点始终来自场景离屏相机。
use crate::{components::SceneEditorViewport, resources::SceneEditorViewportState};
use bevy::{
    ecs::system::SystemParam, prelude::*, ui::RelativeCursorPosition, window::PrimaryWindow,
};

#[derive(SystemParam)]
pub(crate) struct PlanePointer<'w, 's> {
    pub(crate) viewport: Option<Res<'w, SceneEditorViewportState>>,
    canvas: Query<'w, 's, &'static RelativeCursorPosition, With<SceneEditorViewport>>,
    pub(crate) cameras: Query<'w, 's, (&'static Camera, &'static Transform)>,
    pub(crate) windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
}

impl PlanePointer<'_, '_> {
    pub(crate) fn on_plane(&self, y: f32) -> Option<Vec3> {
        let view = self.viewport.as_ref()?;
        let cursor = self.canvas.single().ok()?;
        if !cursor.cursor_over() {
            return None;
        }
        let pixel = (cursor.normalized? + Vec2::splat(0.5)) * view.render_size.as_vec2();
        let (camera, pose) = self.cameras.get(view.camera).ok()?;
        let ray = camera
            .viewport_to_world(&GlobalTransform::from(*pose), pixel)
            .ok()?;
        let distance = ray.intersect_plane(Vec3::Y * y, InfinitePlane3d::new(Vec3::Y))?;
        Some(ray.get_point(distance))
    }

    pub(crate) fn project(&self, world: Vec3) -> Option<Vec2> {
        let (camera, pose) = self.cameras.get(self.viewport.as_ref()?.camera).ok()?;
        camera
            .world_to_viewport(&GlobalTransform::from(*pose), world)
            .ok()
    }
}
