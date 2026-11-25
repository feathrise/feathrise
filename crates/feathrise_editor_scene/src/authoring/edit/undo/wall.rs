//! 沿线围墙整笔历史只在当前摆放、票据与实际栈顶全部匹配时一次发布。

use std::sync::Arc;
use bevy::prelude::Rect;
use feathrise_scene_content::{SceneId, ScenePlacementRecord};
use crate::authoring::{CourtyardBatchChange, courtyards::{CourtyardAuthoringState, PendingVisualSync},
    placement_deletion::PlacementDeletionSource, resources::EditorSceneDraft};
use crate::authoring::placement_append::{capture_prefix, validated_append};
use super::deletion_preparation::PreparedHistory;

pub(super) struct WallHistory {
    records: Vec<ScenePlacementRecord>,
    water_count: usize,
    visuals: PendingVisualSync,
}

impl WallHistory {
    /// 已核对尾段与不可变命令相同，只捕获前缀。全表查重、重做拷贝和世界队列都在同一 worker。
    pub(super) fn prepare(mut records: Vec<ScenePlacementRecord>, water_count: usize, change: Arc<CourtyardBatchChange>,
        source: PlacementDeletionSource, scene: SceneId, bounds: Rect, structure: u64, undoing: bool,
    ) -> Result<PreparedHistory, String> {
        let base = change.wall_base.ok_or("围墙历史缺少原摆放长度")?;
        if base != records.len() {
            return Err("围墙追加时的摆放尾段已经变化".into());
        }
        let water_count = validated_append(&records, &change.added, water_count, undoing, &scene, bounds, None)?;
        if !undoing {
            records.try_reserve_exact(change.added.len()).map_err(|_| "无法分配重做后的围墙摆放记录")?;
            records.extend_from_slice(&change.added);
        }
        let updates = change.added.iter().cloned().enumerate().map(|(i, record)| (base + i, record, !undoing));
        let visuals = PendingVisualSync::prepare(scene, structure.wrapping_add(1), updates);
        let ids = change.added.iter().map(|record| record.id.as_str());
        let deletions = if undoing { source.prepare(ids, std::iter::empty()) }
            else { source.prepare(std::iter::empty(), ids) };
        Ok(PreparedHistory { deletions, wall: Some(Self { records, water_count, visuals }) })
    }

    /// 修订发布前交换完整数组及视觉队列，旧数组交给原单飞槽退役。
    pub(super) fn apply(self, draft: &mut EditorSceneDraft, state: &mut CourtyardAuthoringState) -> Vec<ScenePlacementRecord> {
        state.adopt_visuals(self.visuals);
        draft.placement_water_surface_count = self.water_count;
        std::mem::replace(&mut draft.placements.placements, self.records)
    }
}

/// 草稿持有独占 Vec；每次最多读取一份固定预算。尾段与栈顶逐项比对，避免复制已经共享的墙段。
pub(super) fn capture(records: &mut Vec<ScenePlacementRecord>, cursor: &mut usize, draft: &EditorSceneDraft,
    change: &CourtyardBatchChange, base: usize, undoing: bool, budget: usize) -> Result<bool, String> {
    let source = &draft.placements.placements;
    if base.checked_add(if undoing { change.added.len() } else { 0 }) != Some(source.len()) {
        return Err("围墙追加时的摆放尾段已经变化".into());
    }
    let end = cursor.saturating_add(budget).min(source.len());
    capture_prefix(records, source, base, end)?;
    if end > base {
        let start = (*cursor).max(base);
        if source[start..end] != change.added[start - base..end - base] {
            return Err("围墙尾段的完整成员记录已变化".into());
        }
    }
    *cursor = end;
    Ok(end == source.len())
}
