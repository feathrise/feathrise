//! 显式自有工程验收的唯一状态与后台摘要槽；普通编辑会话不安装此资源。

use super::types::*;
use bevy::{prelude::*, tasks::Task};
use feathrise_map_content::TerrainId;

#[derive(Resource)]
pub(super) struct TerrainWorkflow {
    pub phase: Phase,
    pub domain: Domain,
    pub clock: Clock,
    pub terrain: Option<TerrainId>,
    pub map: Option<feathrise_map_content::MapId>,
    pub view: Option<View>,
    pub pulse: Option<Entity>,
    pub mouse_owned: bool,
    pub tail: bool,
    pub first: Option<Vec3>,
    pub last: Option<Vec3>,
    pub undo_before: usize,
    pub redo_before: usize,
    pub task: Option<Task<Result<Fingerprint, String>>>,
    pub capture_revision: u64,
    pub before: Option<Fingerprint>,
    pub after: Option<Fingerprint>,
    pub history_kind: String,
    pub changed_samples: usize,
    pub undo_verified: bool,
    pub preset_id: Option<String>,
    pub tab: Option<u64>,
    pub surface_layer: Option<String>,
    pub blade_layer: Option<String>,
    pub blade_output: Option<String>,
    pub author_count: usize,
    pub proofs: Vec<StrokeProof>,
    pub failure: Option<String>,
}

impl Default for TerrainWorkflow {
    fn default() -> Self {
        Self {
            phase: Phase::Idle,
            domain: Domain::Height,
            clock: Clock::default(),
            terrain: None,
            map: None,
            view: None,
            pulse: None,
            mouse_owned: false,
            tail: false,
            first: None,
            last: None,
            undo_before: 0,
            redo_before: 0,
            task: None,
            capture_revision: 0,
            before: None,
            after: None,
            history_kind: String::new(),
            changed_samples: 0,
            undo_verified: false,
            preset_id: None,
            tab: None,
            surface_layer: None,
            blade_layer: None,
            blade_output: None,
            author_count: 0,
            proofs: Vec::new(),
            failure: None,
        }
    }
}

impl TerrainWorkflow {
    pub(super) fn advance(&mut self, phase: Phase) {
        self.phase = phase;
        self.clock.phase = std::time::Instant::now();
    }
}
