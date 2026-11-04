//! 手柄的单记录事务；预览与草稿分离，取消恢复当下权威值，提交只登记一次历史。

use super::types::{MoveAxis, MoveDrag, MoveHandle};
use crate::authoring::{
    EditorAuthoringSession, EditorCommand, PlacementTerrainSampler, reconcile_record_with_terrain, update_follow_offset_from_world_y,
};
use crate::authoring::{placement_record_bounds, placement_pose::world_to_source};
use crate::resources::MapEditorState;
use crate::ui::{
    placement_transform::sync_placement_record_world_with_terrain, placement_water::PlacementInstanceIndex,
};
use bevy::{ecs::system::SystemParam, prelude::*};
use feathrise_map_content::MapPlacementRecord;
use feathrise_scene::{MapPlacementInstance, MapPlacementRuntimeStatus};

#[derive(SystemParam)]
pub(crate) struct MoveWorld<'w, 's> {
    pub(super) commands: Commands<'w, 's>,
    assets: Res<'w, AssetServer>,
    pub(super) index: Res<'w, PlacementInstanceIndex>,
    pub(super) runtime_status: Option<Res<'w, MapPlacementRuntimeStatus>>,
    pub(super) roots: Query<'w, 's, (Entity, &'static MapPlacementInstance)>,
    pub(super) group_roots: Query<'w, 's, (&'static Transform, Option<&'static ChildOf>)>,
    pub(super) paste_groups: Query<'w, 's, (), With<crate::authoring::placement_batch::PastePreviewGroup>>,
    pub(super) terrain_queries: feathrise_scene::TerrainObjectQueries<'w, 's>,
    pub(super) terrain_instances: Query<'w, 's, (&'static feathrise_scene::SubMapInstance,
        &'static feathrise_scene::TerrainObjectInstance, &'static feathrise_scene_core::residency::MapSourceFrame)>,
    pub(super) active_batches: Query<'w, 's, (), With<feathrise_scene::SubMapActive>>,
}

impl MoveWorld<'_, '_> {
    pub(super) fn preview(
        &mut self,
        session: &mut EditorAuthoringSession,
        record: &MapPlacementRecord,
    ) -> Result<(), String> {
        let bounds = session
            .draft
            .as_ref()
            .ok_or_else(|| "地图作者草稿尚未就绪".to_owned())
            .and_then(|draft| placement_record_bounds(draft, record))?;
        sync_placement_record_world_with_terrain(
            &mut self.commands,
            &self.assets,
            session,
            &self.index,
            &self.roots,
            record,
            bounds,
            true,
            true,
            &self.terrain_queries,
        )?;
        Ok(())
    }

    pub(super) fn cancel(&mut self, session: &mut EditorAuthoringSession, drag: &MoveDrag) {
        if !drag.preview_applied {
            return;
        }
        let Some(draft) = session
            .draft
            .as_ref()
            .filter(|draft| draft.map_id == drag.map_id)
        else {
            return;
        };
        // 【需求设计】外部操作可能已改值或删除；只恢复现存的当前权威记录，不能覆盖新操作或补生已删根。
        let Some(record) = draft
            .placements
            .placements
            .get(drag.record_index)
            .filter(|record| record.id == drag.before.id)
            .cloned()
        else {
            return;
        };
        let bounds = placement_record_bounds(draft, &record).unwrap_or(drag.source_bounds);
        let restored = sync_placement_record_world_with_terrain(
            &mut self.commands,
            &self.assets,
            session,
            &self.index,
            &self.roots,
            &record,
            bounds,
            true,
            false,
            &self.terrain_queries,
        );
        if restored.is_err() && record.terrain_patch.is_some() {
            // 来源刚被移除/退役时公共入口会拒绝预览；旧地址表不能覆盖已由其他作者接管的实体。
            if let Some(entity) = session.placement_previews.get(&record.id).copied()
                && self.group_roots.get(entity).is_ok()
                && self.roots.get(entity).is_ok_and(|(_, instance)| instance.map_id() == &drag.map_id
                    && instance.id() == record.id && instance.is_active() && instance.is_terrain_patch()) {
                let mut preview = self.commands.entity(entity);
                if let Ok(transform) = record.validated_transform(&drag.map_id, bounds) {
                    preview.insert(transform);
                }
                // 尚存自有根只回滚局部数字；来源无效时隐藏并脱父，不能裸露为错误世界姿态或随旧父退役删除。
                preview.insert(Visibility::Hidden).remove::<ChildOf>();
            }
        }
    }

    pub(super) fn commit(
        &mut self,
        session: &mut EditorAuthoringSession,
        editor: &mut MapEditorState,
        mut drag: MoveDrag,
    ) -> Result<bool, String> {
        let Some(draft) = session
            .draft
            .as_ref()
            .filter(|draft| draft.map_id == drag.map_id)
        else {
            return Err("地图已经变化".into());
        };
        if let Some(binding)=drag.preview.terrain_binding.as_ref() {
            drag.preview.terrain_binding=Some(draft.terrain_ground.as_ref().ok_or("关联地形姿态尚未就绪")?.bind(&binding.terrain_id,feathrise_map_content::TerrainObjectTransform {
                translation:drag.preview.translation,yaw_degrees:drag.preview.yaw_degrees,scale:drag.preview.scale,
                ..Default::default()
            })?);
        }
        let Some(current) = draft
            .placements
            .placements
            .get(drag.record_index)
            .filter(|record| **record == drag.before)
        else {
            return Err("资产已经由其他操作修改".into());
        };
        if current == &drag.preview {
            return Ok(false);
        } // 回到起点或单击不得清空 redo、推进修订或标脏。
        self.preview(session, &drag.preview)?; // 写草稿前完成公共校验和根同步，失败保持原作者值。
        session
            .draft
            .as_mut()
            .expect("已经复核草稿")
            .placements
            .placements[drag.record_index] = drag.preview.clone();
        session.push_bounded_water_history(EditorCommand::PlacementChanged {
            record_index: drag.record_index,
            before: drag.before,
            after: drag.preview,
        });
        session.mark_changed(editor);
        Ok(true)
    }
}

/// 每次候选只改几个标量，失败原样保留上一有效预览且不允许松手提交它。
pub(super) fn update_preview(
    drag: &mut MoveDrag,
    position: Vec3,
    sampler: Option<&PlacementTerrainSampler>,
) -> Result<(), String> {
    let old_position = drag.preview.translation;
    let old_offset = drag.preview.terrain_offset;
    if drag.before.terrain_patch.is_some() {
        // 移动只改变源局部原点，完整线性部分保持原样；避免分解给未编辑的yaw/scale引入舍入。
        let local = if position == drag.world_origin { Vec3::from_array(drag.before.translation) }
            else { drag.frame.as_transform().to_matrix().inverse().transform_point3(position) };
        if !local.is_finite() { return Err("三维地形目标源局部位置无效".into()); }
        drag.preview.translation = local.to_array();
        return Ok(()); // patch不生成普通binding，也不把源局部高度送入世界地面采样。
    }
    drag.preview.translation = position.to_array();
    drag.preview.terrain_offset = drag.before.terrain_offset;
    let result =
        if drag.before.building_preset_id.is_some() || !drag.before.height_mode.follows_terrain() {
            Ok(())
        } else if let Some(sampler) = sampler {
            if drag.handle == MoveHandle::Axis(MoveAxis::Y) {
                update_follow_offset_from_world_y(&mut drag.preview, sampler)
            } else {
                reconcile_record_with_terrain(&mut drag.preview, sampler).map(|_| ())
            }
        } else {
            Err("当前作者地形高度尚未就绪".into())
        };
    let result=result.and_then(|_|sampler.map_or(Ok(()),|sampler|sampler.refresh_record_binding(&mut drag.preview)));
    if result.is_err() {
        drag.preview.translation = old_position;
        drag.preview.terrain_offset = old_offset;
    }
    result
}

/// 世界Y旋转先作用于完整世界矩阵；源局部格式不能表达pitch/roll或剪切时整笔拒绝。
pub(super) fn update_world_rotation(drag: &mut MoveDrag, angle: f32) -> Result<(), String> {
    if angle.abs() <= 1.0e-6 {
        drag.preview.yaw_degrees = drag.before.yaw_degrees;
        drag.preview.scale = drag.before.scale;
        return Ok(());
    }
    let world = Mat4::from_translation(drag.world_origin)
        * Mat4::from_rotation_y(angle.to_radians())
        * Mat4::from_translation(-drag.world_origin) * drag.world_before;
    let pose = world_to_source(world, drag.frame)?;
    drag.preview.yaw_degrees = pose.yaw_degrees;
    drag.preview.scale = pose.scale;
    Ok(())
}
