//! 体素地形的实高只调整对象变换；高度地形没有固定作者高度域。
use feathrise_scene_content::{SceneTerrainKind, SceneTerrainObject};

pub(super) fn voxel_height(object: &SceneTerrainObject) -> Option<f32> {
    (object.terrain.kind == SceneTerrainKind::Voxel).then_some(())?;
    let source = object.voxel.as_ref()?;
    Some((source.max[1] - source.min[1]) * object.transform.scale[1])
}
