//! 静水覆盖笔刷的规则网格包围盒与单样本写入。
//!
//! 【功能设计】线段可以跨多帧扫描，但游标只以“已经访问的包围盒样本数”推进；因此同一
//! 冻结线段在暂停、后台共享和差分检查点后都能从精确位置继续，既不漏点也不重复落笔。

use std::collections::BTreeMap;

use super::WaterDab;
use crate::authoring::paint::brush_overlaps_world_bounds;
use bevy::prelude::Vec3;

/// 一个圆形dab裁剪到规则网格后的行主序访问范围。
#[derive(Clone, Copy)]
pub(super) struct GridBrushBounds {
    min_x: u32,
    min_z: u32,
    row_width: u32,
    row_count: u32,
    step_x: f32,
    step_z: f32,
    center_x: f32,
    center_z: f32,
}

impl GridBrushBounds {
    pub(super) fn len(self) -> usize {
        self.row_width as usize * self.row_count as usize
    }

    fn coordinate(self, local_cursor: usize) -> (u32, u32) {
        let row_width = self.row_width as usize;
        let x = self.min_x + (local_cursor % row_width) as u32;
        let z = self.min_z + (local_cursor / row_width) as u32;
        (x, z)
    }

    fn distance(self, local_cursor: usize) -> f32 {
        let (x, z) = self.coordinate(local_cursor);
        let dx = (x as f32 - self.center_x) * self.step_x;
        let dz = (z as f32 - self.center_z) * self.step_z;
        (dx * dx + dz * dz).sqrt()
    }
}

/// 单个候选样本的结果；历史满时必须在写入前停住同一个游标。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum VisitOutcome {
    Unchanged,
    Changed,
    HistoryFull,
}

pub(super) fn grid_brush_bounds(
    resolution: [u32; 2],
    world_min: [f32; 2],
    world_max: [f32; 2],
    position: Vec3,
    radius: f32,
) -> Option<GridBrushBounds> {
    if resolution[0] < 2
        || resolution[1] < 2
        || !brush_overlaps_world_bounds(position, radius, world_min, world_max)
    {
        return None;
    }
    let step_x = (world_max[0] - world_min[0]) / (resolution[0] - 1) as f32;
    let step_z = (world_max[1] - world_min[1]) / (resolution[1] - 1) as f32;
    if !step_x.is_finite() || !step_z.is_finite() || step_x <= 0.0 || step_z <= 0.0 {
        return None;
    }
    let center_x = (position.x - world_min[0]) / step_x;
    let center_z = (position.z - world_min[1]) / step_z;
    let min_x = (center_x - radius / step_x).floor().max(0.0) as u32;
    let max_x = (center_x + radius / step_x)
        .ceil()
        .min(resolution[0] as f32 - 1.0) as u32;
    let min_z = (center_z - radius / step_z).floor().max(0.0) as u32;
    let max_z = (center_z + radius / step_z)
        .ceil()
        .min(resolution[1] as f32 - 1.0) as u32;
    (min_x <= max_x && min_z <= max_z).then_some(GridBrushBounds {
        min_x,
        min_z,
        row_width: max_x - min_x + 1,
        row_count: max_z - min_z + 1,
        step_x,
        step_z,
        center_x,
        center_z,
    })
}

pub(super) fn apply_mask_visit(
    mask: &mut [u8],
    resolution: [u32; 2],
    bounds: GridBrushBounds,
    local_cursor: usize,
    dab: WaterDab,
    before: &mut BTreeMap<usize, u8>,
    allow_new_history: bool,
) -> VisitOutcome {
    let distance = bounds.distance(local_cursor);
    if distance > dab.radius {
        return VisitOutcome::Unchanged;
    }
    let (x, z) = bounds.coordinate(local_cursor);
    let index = (z * resolution[0] + x) as usize;
    let old = mask[index];
    let target = if dab.erase { 0.0 } else { 255.0 };
    let factor = (dab.amount * (1.0 - distance / dab.radius)).clamp(0.0, 1.0);
    let next = (old as f32 + (target - old as f32) * factor).round() as u8;
    if next == old {
        return VisitOutcome::Unchanged;
    }
    if !before.contains_key(&index) && !allow_new_history {
        return VisitOutcome::HistoryFull;
    }
    before.entry(index).or_insert(old);
    mask[index] = next;
    VisitOutcome::Changed
}
