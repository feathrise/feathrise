use bevy::{prelude::*, tasks::Task};
use feathrise_map_content::{MapId, SparseHeightLiveSnapshot, TerrainId};
use std::{collections::BTreeMap, sync::Arc};

pub struct EmbeddedAuthorPreview {
    pub session: u64,
    pub generation: u64,
    pub sequence: u64,
    pub map_id: String,
    pub bytes: Arc<Vec<u8>>,
}
#[derive(Clone, PartialEq, Eq)]
pub(super) struct Identity {
    pub map: MapId,
    pub session: u64,
    pub generation: u64,
    pub epoch: u64,
}
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Key {
    Extension(u8),
    SubMapPose,
    TerrainPose,
    Mesh,
    Height(Option<TerrainId>),
    Water(Option<TerrainId>, bool),
}
/// 外部作者只投影自己已完成的不可变结果；内容与高频姿态分槽，姿态不能覆盖尚未送出的完整模型。
#[derive(Resource,Default)]
pub struct EmbeddedExternalAuthorVisuals { pub content:Option<ExternalAuthorVisual>,pub pose:Option<ExternalAuthorVisual> }
pub struct ExternalAuthorVisual { pub map:MapId,pub revision:u64,pub domain:String,pub bytes:Arc<Vec<u8>> }
#[derive(Clone, PartialEq, Eq)]
pub(super) struct Version {
    pub revision: u64,
    pub present: bool,
}
pub(super) struct Completion {
    pub identity: Identity,
    pub key: Key,
    pub version: Version,
    pub height: Option<SparseHeightLiveSnapshot>,
    pub result: Result<Arc<EmbeddedAuthorPreview>, String>,
}
#[derive(Resource, Default)]
pub struct EmbeddedAuthorPreviewState {
    pub enabled: bool,
    pub session: u64,
    pub generation: u64,
    pub acknowledged_sequence: u64,
    pub ready: Option<Arc<EmbeddedAuthorPreview>>,
    pub pending: bool,
    pub error: Option<String>,
    pub(super) applied_pose_targets: Option<PoseTargetCredit>,
    pub(super) pending_pose_targets: Option<PoseTargetCredit>,
    pub(super) sub_map_pose: super::poses::PoseMailbox,
    pub(super) terrain_pose: super::poses::PoseMailbox,
    pub(super) identity: Option<Identity>,
    pub(super) epoch: u64,
    pub(super) sequence: u64,
    pub(super) cursor: u8,
    pub(super) acknowledged: BTreeMap<Key, Version>,
    pub(super) heights: BTreeMap<Option<TerrainId>, SparseHeightLiveSnapshot>,
    pub(super) offered: Option<(Key, Version, Option<SparseHeightLiveSnapshot>)>,
    pub(super) failed: Option<(Key, Version)>,
    pub(super) task: Option<Task<Completion>>,
}

impl EmbeddedAuthorPreviewState {
    /// 仅已回执的完整快照能证明游戏已收到该目标来源；本地画布交接不能替代此信用。
    /// 未知新ID留给完整快照先创建，防止实时正文占槽后等待被它挡住的根。
    pub(crate) fn applied_terrain(&self, map: &MapId, id: &TerrainId, session: u64, generation: u64) -> bool {
        self.applied_pose_targets.as_ref().is_some_and(|known| known.session == session
            && known.generation == generation && &known.map_id == map && known.targets.terrains.contains(id))
    }
}

pub(super) struct PoseTargetCredit {
    pub session: u64,
    pub generation: u64,
    pub revision: u64,
    pub map_id: MapId,
    pub targets: Arc<super::super::embedded_snapshot::EmbeddedPoseTargets>,
}
