//! 单个后台编码槽、单个待确认批次；确认后再读取最新生产版本，不以覆盖邮箱漏掉其它地形。

use bevy::{prelude::*, tasks::Task};
use feathrise_scene_content::{SceneId, TerrainId};
use std::{collections::BTreeMap, sync::Arc};

pub struct EmbeddedVegetationPreview {
    pub scene_id: String,
    pub session: u64,
    pub generation: u64,
    pub sequence: u64,
    pub source_revision: u64,
    pub structure_revision: u64,
    pub bytes: Arc<Vec<u8>>,
}

#[derive(Clone, PartialEq, Eq)]
pub(super) struct Source {
    pub scene_id: SceneId,
    pub session: u64,
    pub epoch: u64,
    pub generation: u64,
    pub structure_revision: u64,
    pub terrain_revision: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Version {
    pub published: u64,
    pub layout: u64,
    pub source: u64,
}
pub(super) type CanvasKey = (Option<TerrainId>, bool); // bool区分模型根与旧二维根，两者都没有TerrainId。
pub(super) type Versions = Vec<(CanvasKey, Version)>;
pub(super) type Completion = (Source, Versions, Result<Arc<EmbeddedVegetationPreview>, String>);

#[derive(Resource, Default)]
pub struct EmbeddedVegetationState {
    pub enabled: bool,
    pub session: u64,
    /// 宿主填写当前协议会话的场景代次；默认0不投影。
    pub generation: u64,
    /// 只有实际PreviewApplied才推进；发送成功不能冒充游戏已采用。
    pub acknowledged_sequence: u64,
    pub ready: Option<Arc<EmbeddedVegetationPreview>>,
    pub pending: bool,
    pub error: Option<String>,
    pub(super) source: Option<Source>,
    pub(super) source_epoch: u64,
    pub(super) source_floor: u64,
    pub(super) next_sequence: u64,
    pub(super) acknowledged: BTreeMap<CanvasKey, Version>,
    pub(super) offered: Versions,
    pub(super) failed: Versions,
    pub(super) task: Option<Task<Completion>>,
    pub(super) cursor: Option<Entity>,
    pub(super) root_next: bool,
}
