//! 外部单栋整平暂借作者会话的票据；原作者源始终随完成值归还。
pub(crate) struct SparseTerrainAuthorLease {
    pub(crate) ticket: u64,
    pub(crate) author: super::SparseTerrainAuthorSession,
}

/// 同场景多个对象仍是独立作者源；历史启动和完成都复核稳定对象及源版本路径。
#[derive(Clone, PartialEq, Eq)]
pub(super) struct SparseHistoryTarget {
    pub(super) terrain: Option<feathrise_scene_content::TerrainId>,
    pub(super) source: Option<(Option<feathrise_scene_content::TerrainId>, String)>,
}

pub(super) struct SparseBuildingHistoryRequest {
    pub(super) undoing: bool,
    pub(super) target: SparseHistoryTarget,
    pub(super) change: Box<crate::authoring::commands::BuildingSparseTerrainChange>,
    pub(super) author_revision: u64,
    pub(super) terrain_revision: u64,
    pub(super) structure_revision: u64,
    pub(super) ground_content: Option<[u64; 7]>,
}

pub(super) type SparseBuildingHistoryReady = (
    SparseBuildingHistoryRequest,
    Result<feathrise_scene_content::SparseTerrainPreparedEdit, String>,
);
