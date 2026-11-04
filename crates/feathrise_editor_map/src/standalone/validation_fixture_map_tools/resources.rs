//! 原地图工具有限验收的唯一工作槽；真实地图草稿与笔划继续由原作者资源所有。

use super::types::{MapToolProof, PaintCapture, PaintSourceProof, Phase};
use crate::types::EditorView;
use bevy::{prelude::*, tasks::Task};
use feathrise_map_content::TerrainId;
use std::time::Instant;

#[derive(Resource)]
pub(super) struct MapTools {
    pub(super) phase: Phase,
    pub(super) started: Instant,
    pub(super) phase_started: Instant,
    pub(super) proof: MapToolProof,
    pub(super) pulse: Option<Entity>,
    pub(super) tab: Option<u64>,
    pub(super) terrain: Option<TerrainId>,
    pub(super) undo_before: usize,
    pub(super) vegetation_layer: Option<String>,
    pub(super) mouse_owned: bool,
    pub(super) tail: bool,
    pub(super) first: Option<Vec3>,
    pub(super) last: Option<Vec3>,
    pub(super) vegetation: Option<PaintCapture>,
    pub(super) water: Option<PaintCapture>,
    pub(super) pixels: Option<Task<(PaintSourceProof, PaintSourceProof)>>,
    pub(super) old_view: Option<(EditorView, bool, Vec3, f32, f32, f32)>,
}
