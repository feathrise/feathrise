//! 静态地形手势的真实来源快照；作者姿态相同也不能跨越运行实例代际继续采用旧结果。

use super::{transaction::MoveWorld, types::{PatchRuntimeIdentity, PatchSourceSnapshot}};
use crate::authoring::{EditorMapDraft, placement_record_bounds, placement_pose::placement_record_frame};
use bevy::prelude::*;
use feathrise_map_content::{MapId, MapInstancePath, MapPlacementRecord, MapTerrainKind, TerrainId, TerrainObjectTransform};

impl MoveWorld<'_, '_> {
    pub(super) fn patch_source_snapshot(
        &self, draft: &EditorMapDraft, record: &MapPlacementRecord,
    ) -> Result<Option<PatchSourceSnapshot>, String> {
        let Some(patch) = &record.terrain_patch else { return Ok(None); };
        let id = patch.terrain_id.clone().unwrap_or_else(TerrainId::legacy);
        let frame = placement_record_frame(draft, record)?;
        let runtime = if draft.manifest.terrains.is_some() {
            Some(self.patch_runtime_identity(&draft.map_id, &id, frame)?)
        } else { None };
        Ok(Some(PatchSourceSnapshot { id, frame, bounds: placement_record_bounds(draft, record)?, runtime }))
    }

    pub(super) fn patch_source_current(&self, draft: &EditorMapDraft, source: &PatchSourceSnapshot) -> bool {
        if let Some(objects) = &draft.manifest.terrains {
            let Some(object) = objects.iter().find(|object| object.id == source.id) else { return false; };
            object.enabled && object.terrain.kind == MapTerrainKind::SparseClipmap && object.transform == source.frame
                && Rect::from_corners(object.world_min.into(), object.world_max.into()) == source.bounds
                && source.runtime.as_ref().is_some_and(|old|
                    self.patch_runtime_identity(&draft.map_id, &source.id, source.frame).is_ok_and(|current| &current == old))
        } else {
            source.runtime.is_none() && source.id == TerrainId::legacy()
                && draft.manifest.terrain.kind == MapTerrainKind::SparseClipmap
                && source.frame == TerrainObjectTransform::default()
        }
    }

    fn patch_runtime_identity(
        &self, map: &MapId, id: &TerrainId, frame: TerrainObjectTransform,
    ) -> Result<PatchRuntimeIdentity, String> {
        let parent = self.terrain_queries.instance(map, id).ok_or("关联地形实例尚未就绪")?;
        let (instance, object, actual) = self.terrain_instances.get(parent).map_err(|_| "关联地形实例已退役")?;
        let expected = MapInstancePath::root(map.clone()).terrain(id.clone());
        if &instance.source_map_id != map || instance.path != expected || object.definition.id != *id
            || !object.definition.enabled || object.definition.terrain.kind != MapTerrainKind::SparseClipmap
            || object.definition.transform != frame || !self.active_batches.contains(instance.batch)
            || !same_source_matrix(actual.source_to_world, frame.as_transform().to_matrix())
        {
            return Err("关联地形实例身份或实际姿态已变化".into());
        }
        Ok(PatchRuntimeIdentity { parent, batch: instance.batch, generation: instance.generation,
            source_to_world: actual.source_to_world })
    }
}

/// 大坐标只允许毫米级位移残差；线性轴按自身长度归一比较，不能把真实倾斜/剪切当成缩放舍入。
pub(super) fn same_source_matrix(actual: Mat4, expected: Mat4) -> bool {
    actual.is_finite() && expected.is_finite()
        && actual.w_axis.abs_diff_eq(expected.w_axis, 0.002)
        && [actual.x_axis, actual.y_axis, actual.z_axis].into_iter()
            .zip([expected.x_axis, expected.y_axis, expected.z_axis]).all(|(actual, expected)| {
                let size = expected.truncate().length();
                size > 0.0 && (actual / size).abs_diff_eq(expected / size, 32.0 * f32::EPSILON)
            })
}
