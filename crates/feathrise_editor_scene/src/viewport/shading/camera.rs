//! 原作者控件只发布固定相机参数；附件租约与GPU实现由两种3D方式共同使用。
use super::resources::ShadingState;
use crate::components::SceneEditorCamera;
use bevy::prelude::*;

pub(super) fn sync(
    world: &mut World,
    mut cameras: Local<Option<QueryState<Entity, With<SceneEditorCamera>>>>,
) {
    let mut desired = world.resource::<ShadingState>().view();
    if let Some(request) = world.get_resource::<feathrise_scene::MeshLodInspection>()
        .and_then(|inspection| inspection.request)
    {
        // 复用游戏预览的真实三角线管线，始终绘制已经采用的 Mesh3d，不使用作者边界编码。
        if request.triangles {
            desired = feathrise_scene::author_view::ShadingView::mesh_triangles();
        } else if desired.options.x == 0.0 {
            desired.options.x = 3.0; // 从普通线框进入对照时，关闭三角线即可用同一实体渲染条件测量。
        }
    }
    let entities: Vec<_> = cameras
        .get_or_insert_with(|| world.query_filtered::<Entity, With<SceneEditorCamera>>())
        .iter(world)
        .collect();
    for camera in entities {
        feathrise_scene::author_view::apply_shading(world, camera, Some(desired));
    }
}
