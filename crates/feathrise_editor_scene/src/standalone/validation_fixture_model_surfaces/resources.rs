//! 验收仅保留目标表Arc、有限统计和单次按钮脉冲，不写作者像素或成功状态。

use super::types::Phase;
use bevy::prelude::*;
use feathrise_scene_content::{ModelSurfaceVegetation, SceneId, VegetationPresetDescriptor};
use std::{
    sync::{Arc, Mutex},
    time::Instant,
};

#[derive(Resource)]
pub(super) struct Fixture {
    pub phase: Phase,
    pub started: Instant,
    pub phase_started: Instant,
    pub pulse: Option<Entity>,
    pub scene: Option<SceneId>,
    pub preset: Option<String>,
    pub preset_revision: u64,
    pub registered_presets: Vec<VegetationPresetDescriptor>,
    pub tab: Option<u64>,
    pub layer: Option<String>,
    pub round: usize,
    pub layers_before: usize,
    pub layer_proofs: Vec<serde_json::Value>,
    pub runtime_proof: Option<serde_json::Value>,
    pub lifecycle: super::lifecycle::Lifecycle,
    pub undo_before: usize,
    pub before: Arc<Vec<Arc<ModelSurfaceVegetation>>>,
    pub planted: Option<Arc<Vec<Arc<ModelSurfaceVegetation>>>>,
    pub samples: usize,
    pub planted_sum: u64,
    pub erased_sum: u64,
    pub mouse_owned: bool,
    pub result: Option<Result<serde_json::Value, String>>,
    pub capture: Arc<Mutex<Option<Result<String, String>>>>,
}
impl Default for Fixture {
    fn default() -> Self {
        Self {
            phase: Phase::Idle,
            started: Instant::now(),
            phase_started: Instant::now(),
            pulse: None,
            scene: None,
            preset: None,
            preset_revision: 0,
            registered_presets: Vec::new(),
            tab: None,
            layer: None,
            round: 0,
            layers_before: 0,
            layer_proofs: Vec::new(),
            runtime_proof: None,
            lifecycle: Default::default(),
            undo_before: 0,
            before: Arc::default(),
            planted: None,
            samples: 0,
            planted_sum: 0,
            erased_sum: 0,
            mouse_owned: false,
            result: None,
            capture: Arc::default(),
        }
    }
}
impl Fixture {
    pub fn next(&mut self, phase: Phase) {
        self.phase = phase;
        self.phase_started = Instant::now();
    }
}
