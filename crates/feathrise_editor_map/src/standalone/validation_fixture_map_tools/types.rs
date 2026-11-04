//! 原地图工具有限验收的普通阶段、共享R8快照与序列化证据；不承载ECS资源。

use bevy::prelude::Vec3;
use serde::Serialize;
use std::sync::Arc;

#[derive(Clone, Debug, Serialize)]
pub struct PaintSourceProof {
    pub layer_id: String,
    pub source_path: String,
    pub source_bytes: usize,
    pub source_nonzero: usize,
    pub source_sha256: String,
    pub undo_records: usize,
    pub history_kind: String,
    pub start_world: [f32; 3],
    pub end_world: [f32; 3],
}

/// 模型植被不是Blade草形美术验收；这里只证明同一原R8作者笔刷、后台编译与保存链路。
#[derive(Clone, Debug, Default, Serialize)]
pub struct MapToolProof {
    pub scope: String,
    pub terrain_id: String,
    pub vegetation_preset_id: Option<String>,
    pub vegetation_preset_source: String,
    pub vegetation: Option<PaintSourceProof>,
    pub water: Option<PaintSourceProof>,
    pub water_level: Option<f32>,
    pub complete: bool,
    pub error: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Phase {
    Idle,
    CreatePreset,
    WaitPreset,
    SavePreset,
    WaitPresetSave,
    ClosePreset,
    WaitPresetClose,
    SelectTerrain,
    WaitCanvas,
    CreateVegetation,
    WaitVegetation,
    HoverVegetation,
    PaintVegetation,
    ReleaseVegetation,
    CreateWater,
    WaitWater,
    WaterLevel,
    WaitWaterLevel,
    HoverWater,
    PaintWater,
    ReleaseWater,
    Pixels,
    Complete,
    Failed,
}

#[derive(Clone)]
pub(super) struct PaintCapture {
    pub(super) layer: String,
    pub(super) path: String,
    pub(super) pixels: Arc<Vec<u8>>,
    pub(super) undo: usize,
    pub(super) history_kind: String,
    pub(super) start: Vec3,
    pub(super) end: Vec3,
}
