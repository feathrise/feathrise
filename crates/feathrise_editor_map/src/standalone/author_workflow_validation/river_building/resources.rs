//! 水网/建筑域唯一验收槽，保存有限输入意图及已完成的原作者记录；不自行写作者文件。

use super::types::{Phase, View};
use crate::ui::building_presets::BuildingPlacementControls;
use bevy::prelude::*;
use feathrise_map_content::{MapId, MapPlacementRecord, MapWaterRiver, TerrainId};
use std::{collections::BTreeSet, time::Instant};

#[derive(Resource)]
pub(super) struct Fixture {
    pub(super) phase: Phase,
    pub(super) since: Instant,
    pub(super) frames: usize,
    pub(super) map: Option<MapId>,
    pub(super) terrain: Option<TerrainId>,
    pub(super) view: Option<View>,
    pub(super) controls: Option<BuildingPlacementControls>,
    pub(super) mouse_owned: bool,
    pub(super) point: usize,
    pub(super) undo: usize,
    pub(super) river_ids: BTreeSet<String>,
    pub(super) river: Option<MapWaterRiver>,
    pub(super) placement_ids: BTreeSet<String>,
    pub(super) building: Option<MapPlacementRecord>,
    pub(super) result: Option<Result<serde_json::Value, String>>,
}

impl Default for Fixture {
    fn default() -> Self {
        Self {
            phase: Phase::Idle,
            since: Instant::now(),
            frames: 0,
            map: None,
            terrain: None,
            view: None,
            controls: None,
            mouse_owned: false,
            point: 0,
            undo: 0,
            river_ids: BTreeSet::new(),
            river: None,
            placement_ids: BTreeSet::new(),
            building: None,
            result: None,
        }
    }
}

impl Fixture {
    pub(super) fn next(&mut self, phase: Phase) {
        self.phase = phase;
        self.since = Instant::now();
        self.frames = 0;
    }
}
