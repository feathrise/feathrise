//! Still-water strokes modify only R8 coverage; terrain vertices remain untouched.

use std::sync::Arc; // 后台共享R8时等待释放，地形共享不影响普通刷水。

use bevy::prelude::Vec3; // 水笔刷全部使用世界坐标。
use feathrise_map_content::MapWaterLayer; // 当前静水层的R8覆盖规格。

use super::super::paint::interpolated_dab_count; // 快速拖动以世界距离补齐中间dab。
use super::super::resources::{EditorMapDraft, WaterBrushStroke}; // 普通刷水只写R8作者源。

#[path = "bed_grid.rs"]
mod grid; // R8规则网格包围盒和单像素写入。

use self::grid::{VisitOutcome, apply_mask_visit, grid_brush_bounds};

pub(super) const MAX_WATER_GRID_VISITS_PER_FRAME: usize = 32_768; // R8固定主线程访问预算。
pub(super) const MAX_WATER_STROKE_SAMPLES: usize = 32_768; // 内部临时块硬上限，不划分用户撤销事务。

/// 一次水笔刷使用的规则遮罩与共同世界范围。
#[derive(Clone, Copy)]
struct WaterAuthoringGrid {
    mask_resolution: [u32; 2], // 当前水层R8宽高。
    world_min: [f32; 2],       // X/Z最小角。
    world_max: [f32; 2],       // X/Z最大角。
}

/// 一次世界空间 dab 的冻结输入。
#[derive(Clone, Copy)]
struct WaterDab {
    position: Vec3, // 当前插值后的世界落点。
    radius: f32,    // 遮罩修改的硬半径。
    erase: bool,    // Shift只擦R8。
    amount: f32,    // 当前帧的R8覆盖强度。
}

/// 一次刷段的两份作者源是否真实变化。
#[derive(Clone, Copy, Default)]
pub(super) struct WaterDabChanges {
    pub(super) mask_changed: bool,   // R8至少一个像素变化。
    pub(super) height_changed: bool, // 兼容旧复合历史接口；普通刷水始终为false。
}

impl WaterDabChanges {
    #[cfg(test)]
    pub(super) fn any(self) -> bool {
        self.mask_changed || self.height_changed
    }
}

pub(super) enum WaterSegmentOutcome {
    Progressed {
        changed: WaterDabChanges,  // 本帧局部块的真实变化。
        next_cursor: usize,        // 下一帧从同一冻结线段的精确访问位置继续。
        complete: bool,            // 完整线段完成后才能推进连续鼠标端点。
        checkpoint_required: bool, // 临时块满时回收差分，整笔快照继续保留最初值。
    },
    Deferred, // R8仍被后台消费者持有，本帧不做整图复制。
}

fn checked_grid_len(resolution: [u32; 2]) -> Option<usize> {
    (resolution[0] >= 2 && resolution[1] >= 2)
        .then(|| (resolution[0] as usize).checked_mul(resolution[1] as usize))
        .flatten()
}

fn water_dab(from: Vec3, to: Vec3, dab_index: u32, dab_count: u32, sample: WaterDab) -> WaterDab {
    WaterDab {
        position: from.lerp(to, dab_index as f32 / dab_count as f32),
        ..sample
    }
}

fn validate_sources(
    draft: &EditorMapDraft,
    layer: &MapWaterLayer,
    mask_len: usize,
    sample: WaterDab,
    from: Vec3,
    to: Vec3,
) -> Result<(), String> {
    let expected_mask =
        checked_grid_len(layer.resolution).ok_or_else(|| "河水R8分辨率无效".to_owned())?;
    if mask_len != expected_mask {
        return Err(format!("河水R8为{mask_len}字节，期望{expected_mask}字节"));
    }
    let (min, max) = super::super::water_canvas::layer_bounds(draft, layer);
    let span_x = max[0] - min[0];
    let span_z = max[1] - min[1];
    if !from.is_finite()
        || !to.is_finite()
        || !sample.radius.is_finite()
        || sample.radius <= 0.0
        || !sample.amount.is_finite()
        || sample.amount < 0.0
        || !span_x.is_finite()
        || !span_z.is_finite()
        || span_x <= 0.0
        || span_z <= 0.0
    {
        return Err("水笔刷世界范围、落点、半径或强度无效".to_owned());
    }
    Ok(())
}

fn visit_total(
    grid: WaterAuthoringGrid,
    from: Vec3,
    to: Vec3,
    sample: WaterDab,
    dab_count: u32,
) -> Result<usize, String> {
    let mut total = 0usize;
    for dab_index in 1..=dab_count {
        let dab = water_dab(from, to, dab_index, dab_count, sample);
        total = total
            .checked_add(
                grid_brush_bounds(
                    grid.mask_resolution,
                    grid.world_min,
                    grid.world_max,
                    dab.position,
                    dab.radius,
                )
                .map_or(0, |bounds| bounds.len()),
            )
            .ok_or_else(|| "水笔刷线段访问总数溢出".to_owned())?;
    }
    Ok(total)
}

fn progress(
    changed: WaterDabChanges,
    next_cursor: usize,
    total_visits: usize,
    checkpoint_required: bool,
) -> WaterSegmentOutcome {
    WaterSegmentOutcome::Progressed {
        changed,
        next_cursor,
        complete: next_cursor == total_visits,
        checkpoint_required,
    }
}

/// 冻结线段只分帧刷入R8；不访问GLB顶点，避免普通刷水导致地形塌陷。
#[allow(clippy::too_many_arguments)]
pub(super) fn paint_water_segment(
    draft: &mut EditorMapDraft,
    layer: &MapWaterLayer,
    from: Vec3,
    to: Vec3,
    radius: f32,
    _water_level: f32, // Coverage painting does not derive terrain elevation.
    erase: bool,
    amount: f32,
    cursor: usize,
    stroke: &mut WaterBrushStroke,
) -> Result<WaterSegmentOutcome, String> {
    // 仅在算法入口恢复各轴的世界米数；冻结轨迹仍保存源坐标，分帧游标不随鼠标重解释。
    let metric = super::super::terrain_documents::PaintMetricSpace::from_draft(draft);
    let from = metric.point(from);
    let to = metric.point(to);
    let (min, max) = super::super::water_canvas::layer_bounds(draft, layer);
    let grid = WaterAuthoringGrid {
        mask_resolution: layer.resolution,
        world_min: metric.bounds(min),
        world_max: metric.bounds(max),
    };
    let sample = WaterDab {
        position: to,
        radius,
        erase,
        amount,
    };
    let mask_len = draft
        .water_masks
        .get(&layer.id)
        .map(|mask| mask.len())
        .ok_or_else(|| format!("静水体{}缺少R8作者源", layer.id))?;
    validate_sources(draft, layer, mask_len, sample, from, to)?;
    let dab_count = interpolated_dab_count(from, to, radius);
    let total_visits = visit_total(grid, from, to, sample, dab_count)?;
    if cursor > total_visits {
        return Err(format!(
            "冻结水笔刷游标{cursor}超过线段访问总数{total_visits}"
        ));
    }
    if cursor == total_visits || amount == 0.0 {
        return Ok(progress(
            WaterDabChanges::default(),
            total_visits,
            total_visits,
            false,
        ));
    }
    let mask_shared = draft
        .water_masks
        .get(&layer.id)
        .is_some_and(|mask| Arc::strong_count(mask) > 1);
    if mask_shared {
        // Shared terrain no longer blocks coverage-only painting.
        return Ok(WaterSegmentOutcome::Deferred);
    }

    let mask = Arc::make_mut(
        draft
            .water_masks
            .get_mut(&layer.id)
            .expect("上方已验证稳定水层R8存在"),
    );
    let mut changed = WaterDabChanges::default();
    let mut next_cursor = cursor;
    let mut remaining = MAX_WATER_GRID_VISITS_PER_FRAME;
    let mut absolute_start = 0usize;

    for dab_index in 1..=dab_count {
        let dab = water_dab(from, to, dab_index, dab_count, sample);
        if let Some(bounds) = grid_brush_bounds(
            grid.mask_resolution,
            grid.world_min,
            grid.world_max,
            dab.position,
            dab.radius,
        ) {
            let span_end = absolute_start + bounds.len();
            let mut local = next_cursor.saturating_sub(absolute_start);
            while next_cursor < span_end && local < bounds.len() {
                if remaining == 0 {
                    return Ok(progress(changed, next_cursor, total_visits, false));
                }
                let recorded = stroke.mask_before.len();
                let outcome = apply_mask_visit(
                    mask,
                    grid.mask_resolution,
                    bounds,
                    local,
                    dab,
                    &mut stroke.mask_before,
                    recorded < MAX_WATER_STROKE_SAMPLES,
                );
                remaining -= 1;
                match outcome {
                    VisitOutcome::HistoryFull => {
                        return Ok(progress(changed, next_cursor, total_visits, true));
                    }
                    VisitOutcome::Changed => changed.mask_changed = true,
                    VisitOutcome::Unchanged => {}
                }
                local += 1;
                next_cursor += 1;
            }
            absolute_start = span_end;
        }
    }
    Ok(progress(changed, next_cursor, total_visits, false))
}

#[cfg(test)]
#[path = "bed_tests.rs"]
mod tests;

#[cfg(test)]
pub(crate) fn paint_roundtrip_fixture(draft: &mut EditorMapDraft, layer: &MapWaterLayer) {
    let mut stroke = WaterBrushStroke::default();
    let point = Vec3::new(0.0, layer.water_level, 0.0);
    match paint_water_segment(draft, layer, point, point, 8.0, layer.water_level, false, 1.0, 0, &mut stroke).unwrap() {
        WaterSegmentOutcome::Progressed { changed, complete, .. } => {
            assert!(complete && changed.mask_changed && !changed.height_changed);
            draft.water_dirty_layers.insert(layer.id.clone());
        }
        WaterSegmentOutcome::Deferred => panic!("隔离保存夹具不应仍被历史共享R8写侧"),
    }
}
