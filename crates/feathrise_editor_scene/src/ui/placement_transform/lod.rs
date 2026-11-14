//! 摆放网格的对象LOD策略；显示与碰撞分别覆盖全局，沿完整记录事务保存和撤销。

use bevy::prelude::*;
use feathrise_scene::ScenePlacementInstance;
use feathrise_scene_content::{MeshLodMode, MeshLodPolicy, ScenePlacementRecord};
use super::{PlacementTransformAction as Action, PlacementTransformField as Field, PlacementTransformTarget};
use crate::{authoring::{EditorAuthoringSession, EditorCommand}, resources::SceneEditorState,
    ui::placement_water::PlacementInstanceIndex};

pub(super) fn is_action(action: Action) -> bool {
    matches!(action, Action::DisplayLodInherit | Action::DisplayLodOriginal | Action::DisplayLodCustom
        | Action::CollisionLodInherit | Action::CollisionLodOriginal | Action::CollisionLodCustom)
}

pub(super) fn selected(action: Action, record: &ScenePlacementRecord) -> bool {
    let policy = record.mesh_lod.unwrap_or_default();
    matches!((action, policy.display),
        (Action::DisplayLodInherit, MeshLodMode::Inherit)
        | (Action::DisplayLodOriginal, MeshLodMode::Disabled)
        | (Action::DisplayLodCustom, MeshLodMode::Override(_)))
        || matches!((action, policy.collision),
        (Action::CollisionLodInherit, MeshLodMode::Inherit)
        | (Action::CollisionLodOriginal, MeshLodMode::Disabled)
        | (Action::CollisionLodCustom, MeshLodMode::Override(_)))
}

pub(super) fn field_supported(field: Field, record: &ScenePlacementRecord) -> bool {
    if record.terrain_patch.is_some() { return false; } // 三维地形继续由自身后端管理精度，不展示无效通用参数。
    let policy = record.mesh_lod.unwrap_or_default();
    match field {
        Field::LodScreenError | Field::LodHysteresis => matches!(policy.display, MeshLodMode::Override(_)),
        Field::LodSafeNear | Field::LodCollisionError => matches!(policy.collision, MeshLodMode::Override(_)),
        _ => false,
    }
}

pub(super) fn field_value(field: Field, record: &ScenePlacementRecord) -> f32 {
    let policy = record.mesh_lod.unwrap_or_default();
    match (field, policy.display, policy.collision) {
        (Field::LodScreenError, MeshLodMode::Override(value), _) => value.max_error_pixels,
        (Field::LodHysteresis, MeshLodMode::Override(value), _) => value.hysteresis_ratio,
        (Field::LodSafeNear, _, MeshLodMode::Override(value)) => value.safe_near_meters,
        (Field::LodCollisionError, _, MeshLodMode::Override(value)) => value.max_error_meters,
        _ => 0.0, // 继承和保持原始时字段隐藏，不能把全局当前值伪装成对象自有参数。
    }
}

pub(super) fn set_field_value(field: Field, record: &mut ScenePlacementRecord, value: f32) -> bool {
    if !field_supported(field, record) { return false; }
    let mut policy = record.mesh_lod.unwrap_or_default();
    let target = match (field, &mut policy.display, &mut policy.collision) {
        (Field::LodScreenError, MeshLodMode::Override(settings), _) => &mut settings.max_error_pixels,
        (Field::LodHysteresis, MeshLodMode::Override(settings), _) => &mut settings.hysteresis_ratio,
        (Field::LodSafeNear, _, MeshLodMode::Override(settings)) => &mut settings.safe_near_meters,
        (Field::LodCollisionError, _, MeshLodMode::Override(settings)) => &mut settings.max_error_meters,
        _ => return false,
    };
    if *target == value { return false; }
    *target = value;
    if policy.validate().is_err() { return false; } // 迟到或伪造数值也不能绕过共享精度范围。
    record.mesh_lod = Some(policy);
    true
}

#[allow(clippy::too_many_arguments)]
pub(super) fn apply(
    action: Action, target: &PlacementTransformTarget, before: ScenePlacementRecord,
    commands: &mut Commands, server: &AssetServer, session: &mut EditorAuthoringSession,
    index: &PlacementInstanceIndex, roots: &Query<(Entity, &ScenePlacementInstance)>,
    editor: &mut SceneEditorState, terrain: &feathrise_scene::TerrainObjectQueries,
) -> Result<(), String> {
    if before.terrain_patch.is_some() { return Err("三维地形请使用地形属性中的LOD设置".into()); }
    if selected(action, &before) { return Ok(()); } // 重复选择不重置自定义参数，也不制造历史。
    let mut after = before.clone();
    let mut policy = before.mesh_lod.unwrap_or_default();
    match action {
        Action::DisplayLodInherit => policy.display = MeshLodMode::Inherit,
        Action::DisplayLodOriginal => policy.display = MeshLodMode::Disabled,
        Action::DisplayLodCustom => policy.display = MeshLodMode::Override(Default::default()),
        Action::CollisionLodInherit => policy.collision = MeshLodMode::Inherit,
        Action::CollisionLodOriginal => policy.collision = MeshLodMode::Disabled,
        Action::CollisionLodCustom => policy.collision = MeshLodMode::Override(Default::default()),
        _ => return Err("无效网格LOD操作".into()),
    }
    // 两通道均继承时恢复缺省表示；只改当前通道，不将全局偏好复制进地图。
    after.mesh_lod = (policy != MeshLodPolicy::default()).then_some(policy);
    let draft = session.draft.as_ref().filter(|draft| draft.scene_id == target.scene_id)
        .ok_or("网格LOD所属场景已经变化")?;
    let bounds = crate::authoring::placement_record_bounds(draft, &after)?;
    after.validated_transform(&draft.scene_id, bounds)?;
    let current = session.draft.as_mut().and_then(|draft| draft.placements.placements.get_mut(target.record_index))
        .filter(|record| **record == before).ok_or("网格LOD选择或作者记录已经变化")?;
    *current = after.clone();
    if let Err(error) = super::sync_placement_record_world_with_terrain(
        commands, server, session, index, roots, &after, bounds, true, true, terrain,
    ) {
        if let Some(current) = session.draft.as_mut().and_then(|draft| draft.placements.placements.get_mut(target.record_index)) {
            *current = before;
        }
        return Err(error);
    }
    session.push_bounded_water_history(EditorCommand::PlacementChanged { record_index: target.record_index, before, after });
    session.mark_changed(editor);
    editor.status = "网格LOD策略已更新；显示和碰撞独立，可撤销并随场景保存".into();
    Ok(())
}
