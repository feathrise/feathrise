//! 退休Heightfield路线的宿主模态资源兼容合同；不再监控、雕刻或保存独立GLB高度源。

use bevy::prelude::*;
use feathrise_scene_content::SceneId;

#[derive(Clone, Debug)]
pub(crate) struct TerrainExternalConflict {
    pub(crate) scene_id: SceneId,
    pub(crate) digest: [u8; 32],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TerrainConflictResolution {
    AdoptBlender,
    OverwriteWithF12,
}

/// 仅保留宿主现有输入门及保存失败UI使用的小型状态；普通模型由scene.glb热更新链处理。
#[derive(Resource, Default)]
pub struct TerrainExternalChangeState {
    pub(crate) conflict: Option<TerrainExternalConflict>,
    pub(crate) resolution: Option<TerrainConflictResolution>,
}

impl TerrainExternalChangeState {
    pub(crate) fn register_save_conflict(&mut self, scene_id: SceneId, digest: [u8; 32]) {
        self.conflict = Some(TerrainExternalConflict { scene_id, digest });
        self.resolution = None;
    }

    pub fn modal_open(&self) -> bool { self.conflict.is_some() }
}
