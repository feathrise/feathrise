//! 多笔建筑追加共享的摆放快照、每帧捕获预算和追加预检。

use std::collections::HashSet;
use bevy::prelude::Rect;
use feathrise_scene_content::{SceneId, ScenePlacementRecord, SceneTerrainObject};
use super::resources::EditorSceneDraft;

pub(super) const CAPTURE_BUDGET: usize = 128;

#[derive(PartialEq)]
pub(super) struct PlacementSource {
    allocation: usize,
    pub(super) len: usize,
    pub(super) water_count: usize,
    pub(super) bounds: Option<Rect>,
}

impl PlacementSource {
    pub(super) fn of(draft: &EditorSceneDraft) -> Self {
        let records = &draft.placements.placements;
        Self { allocation: records.as_ptr() as usize, len: records.len(),
            water_count: draft.placement_water_surface_count, bounds: draft.manifest.world_bounds() }
    }
}

/// 只从独占草稿复制本帧范围；一次预留容量，后续帧不再搬移整个前缀。
pub(super) fn capture_prefix(prefix: &mut Vec<ScenePlacementRecord>, source: &[ScenePlacementRecord],
    base: usize, end: usize) -> Result<(), String> {
    prefix.try_reserve_exact(base - prefix.len()).map_err(|_| "无法分配摆放记录快照")?;
    if prefix.len() < end.min(base) { prefix.extend_from_slice(&source[prefix.len()..end.min(base)]); }
    Ok(())
}

/// 追加、撤销与重做共用稳定 ID、变换和水绑定规则；原位编辑没有新增 ID 时跳过整图扫描。
pub(super) fn validated_append(prefix: &[ScenePlacementRecord], added: &[ScenePlacementRecord],
    mut water_count: usize, undoing: bool, scene: &SceneId, bounds: Rect,
    objects: Option<&[SceneTerrainObject]>) -> Result<usize, String> {
    if added.is_empty() { return Ok(water_count); }
    let mut ids = HashSet::with_capacity(added.len());
    for record in added {
        if record.id.is_empty() || !ids.insert(record.id.as_str()) {
            return Err("院落整批历史包含空白或重复的新增实例 ID".into());
        }
        record.validated_transform(scene, super::edit::placement_record_bounds_by_source(bounds, objects, record)?)?;
        record.validate_water_surfaces()?;
        water_count = if undoing { water_count.checked_sub(record.water_surfaces.len()) }
            else { water_count.checked_add(record.water_surfaces.len()) }
            .ok_or("院落整批历史的水绑定计数溢出或不足")?;
    }
    if prefix.iter().any(|record| ids.contains(record.id.as_str())) {
        return Err("院落新增成员 ID 已被其他摆放占用".into());
    }
    Ok(water_count)
}
