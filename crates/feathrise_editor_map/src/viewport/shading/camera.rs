//! 原作者控件只发布固定相机参数；附件租约与GPU实现由两种3D方式共同使用。
use super::{components::ShadingView, resources::ShadingState};
use crate::components::MapEditorCamera;
use bevy::prelude::*;

pub(super) fn sync(
    world: &mut World,
    mut cameras: Local<Option<QueryState<Entity, With<MapEditorCamera>>>>,
) {
    let state = world.resource::<ShadingState>();
    let desired = ShadingView {
        options: Vec4::new(
            state.mode.index(),
            state.cavity as u8 as f32,
            state.normals as u8 as f32,
            state.background as u8 as f32,
        ),
        wire: Vec4::new(state.xray as u8 as f32, state.studio as u8 as f32, 0., 0.),
    };
    let entities: Vec<_> = cameras
        .get_or_insert_with(|| world.query_filtered::<Entity, With<MapEditorCamera>>())
        .iter(world)
        .collect();
    for camera in entities {
        feathrise_scene::author_view::apply_shading(world, camera, Some(desired));
    }
}
