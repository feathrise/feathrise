//! 批量操作的不可变输入与CPU完成协议；跨场景剪贴板持有完整作者记录及三维地形源框架。
use crate::authoring::buildings::{BuildingGroundQuery, CourtyardBuildingValidator};
use crate::authoring::{CourtyardBatchChange, EditorAuthoringSession, PlacementTerrainSampler};
use crate::authoring::placement_deletion::{PlacementDeletionIdentity, PreparedPlacementDeletions};
use crate::viewport::{multi_select::SelectedMember, placement_move::MoveCamera};
use bevy::prelude::*;
use feathrise_scene_content::{BuildingPresetDescriptor, SceneId, ScenePlacementRecord, SceneTerrainObject, TerrainObjectTransform};
use std::sync::Arc;

pub(super) const RECORD_BUDGET: usize = 16;
pub(super) const MAX_CLIPBOARD_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlacementBatchAction {
    Copy,
    Paste,
    Duplicate,
    Delete,
}

#[derive(Clone)]
pub(super) struct BatchIdentity {
    pub scene_id: SceneId,
    pub document: PlacementDeletionIdentity, // 同图重开也必须换弱文档身份，跨帧捕获不能借用新会话的源表。
    pub author: u64,
    pub structure: u64,
    pub terrain: u64,
    pub placement_derived_pose_revision: u64, // 同代贴地仍会改变跨帧捕获的完整姿态，不能发布混合快照。
    pub content: Option<[u64; 7]>,
}
impl BatchIdentity {
    pub fn from_session(
        session: &EditorAuthoringSession,
        content: Option<[u64; 7]>,
    ) -> Option<Self> {
        Some(Self {
            scene_id: session.draft.as_ref()?.scene_id.clone(),
            document: session.placement_deletions.identity(),
            author: session.revision,
            structure: session.placement_structure_revision,
            terrain: session.terrain_revision,
            placement_derived_pose_revision: session.placement_derived_pose_revision,
            content,
        })
    }
}

pub(super) struct ClipboardData {
    pub records: Vec<ScenePlacementRecord>,
    // 三维地形记录仍保存源局部姿态；独立冻结父变换，切换场景或关闭源文档后也能恢复完整世界矩阵。
    pub patch_frames: Arc<Vec<Option<TerrainObjectTransform>>>,
    pub origin: Vec3,
    pub source_scene: SceneId,
}
pub(super) struct Capture {
    pub identity: BatchIdentity,
    pub action: PlacementBatchAction,
    pub members: Arc<Vec<SelectedMember>>,
    pub records: Vec<(usize, ScenePlacementRecord)>,
    pub bounds: Rect,
    pub source_objects: Arc<[SceneTerrainObject]>,
    pub length: usize,
    pub selection_revision: u64,
    pub selected_id: Option<String>,
}

#[derive(Clone, Copy, PartialEq)]
pub(super) struct RayKey {
    pub camera: MoveCamera,
    pub normalized: Vec2,
    pub ui_size: Vec2,
    pub snap: feathrise_editor_core::EditorTranslationSnap, // 共同原点的吸附规则属于每次真实射线身份。
}
pub(super) struct PasteSession {
    pub identity: BatchIdentity,
    pub source: Arc<ClipboardData>,
    pub records: Arc<Vec<ScenePlacementRecord>>,
    pub parent: Entity,
    pub parent_key: String,
    pub roots: Vec<Entity>,
    pub frames: Vec<Option<Entity>>,
    pub patch_target: Option<PatchTarget>,
    pub budget: PatchBudget,
    pub budget_cursor: usize,
    pub cursor: usize,
    pub builtin_only: bool, // 随固定预算准备增量确认；只为纯立方体组开放显式空图工作平面。
    pub point: Option<(RayKey, Vec3)>,
    pub desired: Option<RayKey>,
    pub started: Option<(RayKey, Option<(Option<u64>, bool)>)>,
    pub clicked: Option<RayKey>,
    pub validating: bool,
    pub prepared: Option<PreparedPaste>,
    pub applying: usize,
    pub building_revision: Option<u64>,
    pub presets: Vec<Option<BuildingPresetDescriptor>>,
    pub frozen_presets: Option<Arc<Vec<Option<BuildingPresetDescriptor>>>>,
}

pub(super) struct PasteWork {
    pub source: Arc<Vec<ScenePlacementRecord>>,
    pub delta: Vec3,
    pub records: Vec<ScenePlacementRecord>,
    pub validator: Option<CourtyardBuildingValidator>,
    pub presets: Arc<Vec<Option<BuildingPresetDescriptor>>>,
    pub sampler: Option<PlacementTerrainSampler>,
    pub work_plane: bool, // 主线程已证明目标零地形，worker才可把跨场景跟随语义转为固定高度。
    pub patch: Option<PatchPasteContext>,
    pub query: Option<BuildingGroundQuery>,
    pub bounds: Rect,
    pub scene_id: SceneId,
    pub building_revision: u64,
}

#[derive(Clone)]
pub(super) struct PatchTarget {
    pub object: Arc<SceneTerrainObject>,
    // None是未命中/不支持；旧主Clipmap必须有显式身份，不能用空TerrainId猜测成功。
    pub legacy: bool,
}

#[derive(Clone, Copy, Default)]
pub(super) struct PatchBudget {
    pub roots: usize,
    pub holes: usize,
}

pub(super) struct PatchPasteContext {
    pub frames: Arc<Vec<Option<TerrainObjectTransform>>>,
    pub target: PatchTarget,
    pub existing: PatchBudget,
}

pub(super) struct CancelledBatch {
    pub identity: BatchIdentity,
    pub _capture: Option<Capture>,
    pub _waiting: Option<Box<PasteWork>>,
    pub _paste: Option<PasteSession>,
}

pub(super) struct PreparedPaste {
    pub apply: Vec<ScenePlacementRecord>,
    pub history: CourtyardBatchChange,
    pub members: Vec<SelectedMember>,
    pub water_count: usize,
}
pub(super) enum BatchOutput {
    Captured(Result<Arc<ClipboardData>, String>, bool),
    Deleted(Result<(CourtyardBatchChange, Vec<(usize, ScenePlacementRecord, bool)>, PreparedPlacementDeletions), String>),
    Ghosts(Result<Arc<Vec<ScenePlacementRecord>>, String>),
    Ray(RayKey, feathrise_scene::ClipmapGroundHit, Option<PatchTarget>),
    Paste(Result<PreparedPaste, String>),
    Waiting(Box<PasteWork>),
    Retired,
}
pub(super) struct BatchJob {
    pub identity: BatchIdentity,
    pub ticket: u64,
    pub task: bevy::tasks::Task<BatchOutput>,
    pub selection: Option<(u64, Option<String>)>,
}
