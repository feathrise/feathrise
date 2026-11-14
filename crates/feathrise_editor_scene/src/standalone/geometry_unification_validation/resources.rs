//! 显式隔离工程验收的唯一状态、后台读取槽和固定规模快照。

use super::types::*;
use bevy::{prelude::*, tasks::Task};
use feathrise_scene_content::{SceneId, SceneTerrainObject, TerrainId, VegetationPresetDescriptor};
use std::{collections::BTreeSet, sync::{Arc, Mutex}, time::Instant};

/// 目标三维草窗使用独立回读票据；相机、字段、源纹理或完整采用代变化均撤销旧信用。
#[derive(Resource)]
pub(super) struct VolumeGpuGate {
    pub identity: VolumeCaptureIdentity,
    pub ticket: u64,
    pub baseline: u64,
    pub started: Instant,
    pub last_diagnostic: Option<Instant>,
    pub was_ready: bool,
    pub previous: feathrise_environment::vegetation::blade::GpuBladeReadbackConfig,
}

#[derive(Resource)]
pub(super) struct GeometryFixture {
    pub phase: Phase,
    pub creation: Creation,
    pub domain: Domain,
    pub scene: Option<SceneId>,
    pub tiny: Option<TerrainId>,
    pub small: Option<TerrainId>,
    pub huge: Option<TerrainId>,
    pub voxel: Option<TerrainId>,
    pub ids_before: BTreeSet<TerrainId>,
    pub lod_count: usize,
    pub rejection_before: Option<(u64, SceneTerrainObject)>,
    pub task: Option<Task<Result<GeometrySourceProof, String>>>,
    pub hover_query: Option<Task<Result<serde_json::Value, String>>>,
    pub hover_query_captured: bool,
    pub capture_revision: u64,
    pub save_before: u64,
    pub capture: Arc<Mutex<Option<Result<(), String>>>>,
    pub stable_frames: u8,
    pub before: Option<GeometrySourceProof>,
    pub after: Option<GeometrySourceProof>,
    pub undo_before: usize,
    pub undo_verified: bool,
    pub author_count: usize,
    pub layer_preset_tab: Option<u64>,
    pub layer_preset_source: Option<String>,
    pub layer_preset_revision: u64,
    pub layer_preset_ids: BTreeSet<String>,
    /// 后续场景工具只允许原目录与本轮真实保存的新Blade，不把同来源的任意预设当作夹具所有。
    pub layer_preset_before: Vec<VegetationPresetDescriptor>,
    pub layer_preset_adopted: Option<VegetationPresetDescriptor>,
    pub pulse: Option<Entity>,
    pub mouse_owned: bool,
    pub hit: Option<Vec3>,
    pub started: Instant,
    pub phase_started: Instant,
    pub view: Option<(crate::types::EditorView, bool, Vec3, f32, f32, f32)>,
    pub proof: GeometryUnificationProof,
}

impl Default for GeometryFixture {
    fn default() -> Self {
        Self {
            phase: Phase::Idle, creation: Creation::Tiny, domain: Domain::Height,
            scene: None, tiny: None, small: None, huge: None, voxel: None,
            ids_before: Default::default(), lod_count: 0, rejection_before: None,
            task: None, hover_query: None, hover_query_captured: false,
            capture_revision: 0, save_before: 0, capture: Default::default(), stable_frames: 0, before: None, after: None,
            undo_before: 0, undo_verified: false, author_count: 0,
            layer_preset_tab: None, layer_preset_source: None, layer_preset_revision: 0, layer_preset_ids: Default::default(),
            layer_preset_before: Vec::new(), layer_preset_adopted: None,
            pulse: None, mouse_owned: false, hit: None,
            started: Instant::now(), phase_started: Instant::now(), view: None,
            proof: GeometryUnificationProof::default(),
        }
    }
}

impl GeometryFixture {
    pub fn advance(&mut self, phase: Phase) {
        if phase == Phase::Hover { self.hover_query_captured = false; }
        self.phase = phase; self.phase_started = Instant::now();
        eprintln!("几何统一实窗验收：{:?}/{:?}/{:?}", self.creation, self.domain, phase);
    }
    pub fn target(&self) -> Option<&TerrainId> {
        match self.domain { Domain::Height | Domain::HeightVegetation => self.small.as_ref(), Domain::Voxel | Domain::VoxelUnderside => self.voxel.as_ref() }
    }
    pub fn center(&self) -> Vec3 {
        match self.domain { Domain::Height | Domain::HeightVegetation => Creation::Small.origin(),
            Domain::Voxel => Creation::Voxel.origin(), Domain::VoxelUnderside => Creation::Voxel.origin() - Vec3::Y * 0.05 }
    }
}
