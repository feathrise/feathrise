//! 密度源历史只浅共享；派生网格和文件字节不进入撤销栈。
use bevy::prelude::*;
use bevy::tasks::Task;
use feathrise_scene_content::{SceneId, ScenePlacementRecord, SceneTerrainCutout};
use feathrise_terrain::volume::{
    VolumeBrush,
    sparse::{VolumeMemoryBudget, VolumeSource},
};
use feathrise_scene::TerrainVolumePreviewTarget;
use std::sync::{Arc, Weak};

pub(super) const HISTORY_BYTES: usize = 64 * 1024 * 1024;
pub(super) const RECORD_SCAN: usize = 128;

#[derive(Clone)]
pub(crate) struct VolumeSnapshot {
    pub record: ScenePlacementRecord,
    pub source: Arc<VolumeSource>,
}
#[derive(Clone)]
/// 外部作者只携带该历史值；内容和创建仍由普通编辑器自己的受检事务掌握。
pub struct VolumeSculptChange {
    pub(crate) scene_id: SceneId,
    pub(crate) index: usize,
    pub(crate) before: Option<VolumeSnapshot>,
    pub(crate) after: VolumeSnapshot,
}
#[derive(Clone)]
pub(crate) struct VolumeSaveFile {
    pub placement_id: String,
    pub path: String,
    pub bytes: Arc<Vec<u8>>,
}

pub(super) struct PreparedVolume {
    pub snapshot: VolumeSnapshot,
    pub render_record: ScenePlacementRecord,
    pub resident_window: Option<feathrise_scene::StreamedVolumeResident>,
    pub bytes: Arc<Vec<u8>>,
    pub meshes: Vec<Mesh>,
    pub fields: feathrise_scene::VolumeSurfaceFields,
}
pub(super) enum JobOutput {
    Prepared(Box<PreparedVolume>),
    Staged(Box<SpatialSculptTransaction>), // 私有索引根进度；不能发布实体、场景记录或历史。
    Opened(VolumeSnapshot),
    Unchanged { at_boundary: bool },
}
pub(super) struct Work {
    pub task: Task<Result<JobOutput, String>>,
    pub scene_id: SceneId,
    pub revision: u64,
    pub source: Option<Arc<VolumeSource>>,
    pub index: usize,
    pub history: Option<(bool, Box<VolumeSculptChange>)>,
    pub ground: Option<[u64; 7]>,
    pub opening: bool,
    pub spatial: Option<crate::viewport::spatial_brush::SpatialBrushRequest>,
}
/// 确认式长笔划只驻留一个私有版本与一个批次；基础作者源保持原样直到整笔完成。
pub(super) struct SpatialSculptTransaction {
    pub request: crate::viewport::spatial_brush::SpatialBrushRequest,
    pub snapshot: VolumeSnapshot,
    pub batches: SpatialBatches,
    pub batch: Option<feathrise_terrain::volume::VolumeBrushBatch>,
    pub ground: [u64; 7],
    pub changed: bool,
}
pub(super) struct Adoption {
    pub root: Entity,
    pub revision: u64,
    pub index: usize,
    pub next: Option<VolumeSnapshot>,
    pub bytes: Option<Arc<Vec<u8>>>,
    pub previous: Option<TerrainVolumePreviewTarget>,
    pub history: Option<(bool, Box<VolumeSculptChange>)>,
    pub started: f64,
    pub expected: Option<ScenePlacementRecord>,
    pub spatial: Option<crate::viewport::spatial_brush::SpatialBrushRequest>,
}

pub(super) enum SpatialBatches {
    Line(feathrise_terrain::volume::VolumeBrushBatches),
    Path(feathrise_terrain::volume::VolumePathBatches),
}
pub(super) struct Stroke {
    pub spatial: Option<crate::viewport::spatial_brush::SpatialBrushRequest>, // 保留整段和场景/作者/高度/结构身份，缺页等待也不能冒领新源。
    pub shape: feathrise_terrain::volume::VolumeBrushShape,
    pub half_height: f32,
    pub lock_height: bool,
    pub flatten_height: Option<f32>, // 首次真实落点冻结；缺页、打开旧源和后台分段都不得重采样。
    pub field: Option<(
        feathrise_terrain::volume::sparse::paged::VolumeFieldBrush,
        Vec3,
        Vec3,
    )>,
    pub tool: crate::types::EditorTool,
    pub revision: u64,
    pub before: Option<VolumeSnapshot>,
    pub index: usize,
    pub released: bool,
    pub last: Option<Vec3>,
    pub tail: Option<(Vec3, VolumeBrush)>,
    pub first_click_done: bool,
    pub seed: Option<(ScenePlacementRecord, f32, VolumeBrush)>,
    pub new_region: bool,
    pub history_slot: Option<usize>, // 首次完整采用即登记；后续段仅更新本笔末值。
    pub started: f64,
}
#[derive(Default)]
pub(super) struct RecordIndex {
    pub identity: Option<(SceneId, u64)>,
    pub next: usize,
    pub ready: bool,
    pub error: Option<String>,
    pub records: Vec<(usize, ScenePlacementRecord, Option<SceneTerrainCutout>)>, // 至多32个三维根。
}

#[derive(Resource, Default)]
pub(crate) struct VolumeAuthoringState {
    pub(super) scope: Option<SceneId>,
    pub(super) current: Option<VolumeSnapshot>,
    pub(super) current_index: usize,
    pub(super) root: Option<Entity>,
    pub(super) stroke: Option<Stroke>,
    pub(super) work: Option<Work>,
    pub(super) spatial_transaction: Option<SpatialSculptTransaction>,
    pub(super) adoption: Option<Adoption>,
    pub(super) release: Option<Task<()>>,
    pub(super) index: RecordIndex,
    pub(super) sources: Vec<Weak<VolumeSource>>, // 弱引用仅计唯一源，不延长历史寿命。
    pub(super) holds: Vec<Arc<VolumeSource>>, // 保证通用历史清空不会在主帧成为大密度数组的最后释放者。
    pub(super) budget: Arc<VolumeMemoryBudget>, // 所有块/缓存共享真实分配额度，Arc历史重复引用不重复计费。
    pub(super) files: Vec<VolumeSaveFile>,      // 最多32条Arc文件；保存前按稳定ID及路径复核。
    pub(super) unsaved_region: Option<String>,
    pub(super) saved_sequence: u64,
    pub(super) retired_files: Vec<VolumeSaveFile>,
}
impl VolumeAuthoringState {
    /// 复用已完成的体积索引判定二维快刷资格，不在每个刷印上重扫任意数量的摆放记录。
    pub(crate) fn planar_density_refresh_available(&self, session: &crate::authoring::EditorAuthoringSession) -> bool {
        let Some(draft) = session.draft.as_ref() else { return false; };
        !self.busy() && self.index.ready && self.index.error.is_none() && self.index.records.is_empty()
            && self.index.identity.as_ref().is_some_and(|(map, revision)| map == &draft.scene_id
                && *revision == session.placement_structure_revision)
    } // 有洞壁/体积继承时仍采用完整植被来源，不能只更新室外草窗而留下旧三维密度。

    pub(crate) fn retarget_terrain_canvas(&mut self) {
        self.current = None; self.current_index = 0; self.root = None; self.index = RecordIndex::default();
        // 文件和历史属于整个场景，切换画布只丢弃可重建的当前拾取缓存。
    }
    pub(crate) fn flatten_height(&self) -> Option<f32> {
        self.stroke
            .as_ref()
            .filter(|s| s.tool == crate::types::EditorTool::VolumeFlatten)
            .and_then(|s| s.flatten_height)
    }

    pub(crate) fn brush_height(&self) -> Option<f32> {
        self.stroke.as_ref().and_then(|s| s.flatten_height)
    }

    pub(crate) fn busy(&self) -> bool {
        self.stroke.is_some()
            || self.work.is_some()
            || self.spatial_transaction.is_some()
            || self.adoption.is_some()
            || self.release.is_some()
    }
    pub(crate) fn save_files(&self) -> Vec<VolumeSaveFile> {
        self.files.clone()
    }
    pub(super) fn reserve_source(&mut self) -> Result<(), String> {
        self.sources.retain(|source| source.strong_count() > 0);
        let bytes: usize = self
            .holds
            .iter()
            .filter(|s| Arc::strong_count(s) > 1 && !s.is_sparse())
            .map(|s| s.memory_bytes())
            .sum();
        let one = 65usize.pow(3) * 2 + 65usize.pow(2) * 4;
        if bytes + self.budget.used_bytes() + one > HISTORY_BYTES || self.sources.len() >= 128 {
            return Err("三维雕刻历史达到64MiB预算；请结束当前会话后重新打开场景".into());
        }
        self.budget.reserve_legacy_bytes(bytes + one)?;
        Ok(())
    }
    pub(super) fn reserve_restore(&mut self) -> Result<(), String> {
        // 历史源已被计入额度，恢复只派生网格/文件；不能拿“下一份源”的额度阻止撤销。
        let bytes = self
            .holds
            .iter()
            .filter(|s| Arc::strong_count(s) > 1 && !s.is_sparse())
            .map(|s| s.memory_bytes())
            .sum();
        self.budget.reserve_legacy_bytes(bytes)
    }
    pub(super) fn register_source(&mut self, source: &Arc<VolumeSource>) {
        // 持久版本只有固定大小的索引根，历史不占密度缓存额度，也不受旧128份源门限制。
        if matches!(source.as_ref(), VolumeSource::Paged(_)) {
            return;
        }
        if !self
            .sources
            .iter()
            .any(|old| old.as_ptr() == Arc::as_ptr(source))
        {
            self.sources.push(Arc::downgrade(source));
            self.holds.push(source.clone());
        }
    }
}
