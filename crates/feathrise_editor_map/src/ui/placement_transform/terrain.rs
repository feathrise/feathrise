//! 三维地形角色和洞口属于同一摆放记录；整条历史可撤销，保存后联合采用。

use super::{
    PlacementTransformAction, PlacementTransformTarget, actions::sync_placement_record_world_with_terrain,
};
use crate::{
    authoring::{EditorAuthoringSession, EditorCommand},
    resources::MapEditorState,
    ui::placement_water::PlacementInstanceIndex,
};
use bevy::prelude::*;
use feathrise_map_content::{
    MapPlacementHeightMode, MapPlacementRecord, MapPlacementTerrainPatch, MapTerrainCutout,
    MapTerrainKind,
};

fn record_pose(record: &MapPlacementRecord) -> feathrise_map_content::TerrainObjectTransform {
    feathrise_map_content::TerrainObjectTransform { translation: record.translation,
        yaw_degrees: record.yaw_degrees, scale: record.scale, ..Default::default() }
}

fn set_pose(record: &mut MapPlacementRecord, pose: feathrise_map_content::TerrainObjectTransform) {
    record.translation = pose.translation;
    record.yaw_degrees = pose.yaw_degrees;
    record.scale = pose.scale;
}

/// 分类切换是坐标合同切换：候选先按完整矩阵换算，不能把源局部数字直接当成世界数字。
fn configure_in_draft(
    action: PlacementTransformAction, before: &MapPlacementRecord,
    draft: &crate::authoring::EditorMapDraft, selected: Option<&feathrise_map_content::TerrainId>,
) -> Result<MapPlacementRecord, String> {
    use crate::authoring::placement_pose::{placement_record_frame, source_to_world, world_to_source};
    let enabling = before.terrain_patch.is_none();
    let target_id = if let Some(patch) = &before.terrain_patch {
        patch.terrain_id.clone()
    } else if draft.manifest.terrains.is_some() {
        Some(before.terrain_binding.as_ref().map(|binding| &binding.terrain_id).or(selected)
            .cloned().ok_or("请先选择用于三维地形的启用Clipmap网格")?)
    } else { None };
    let mut source_record = before.clone();
    source_record.terrain_binding = None;
    source_record.terrain_patch = Some(MapPlacementTerrainPatch {
        terrain_id: target_id.clone(), ..Default::default()
    });
    let frame = placement_record_frame(draft, &source_record)?;
    let mut after = configure(action, before, MapTerrainKind::SparseClipmap)?;
    match action {
        PlacementTransformAction::ToggleTerrainPatch if enabling => {
            let local = world_to_source(record_pose(before).as_transform().to_matrix(), frame)?;
            set_pose(&mut after, local);
            after.terrain_binding = None; // 补丁使用独立关联字段，不能同时保留普通内容绑定。
            after.terrain_patch.as_mut().expect("分类已启用").terrain_id = target_id;
        }
        PlacementTransformAction::ToggleTerrainPatch => {
            let world = source_to_world(record_pose(before), frame)?;
            let pose = world_to_source(world, Default::default())?;
            set_pose(&mut after, pose);
            // 只有可由普通记录完整表达的世界姿态才关闭分类；不借绑定隐藏丢失的倾斜或剪切。
            after.terrain_binding = if draft.manifest.terrains.is_some() {
                Some(feathrise_map_content::MapTerrainBinding::capture(
                    target_id.unwrap_or_else(feathrise_map_content::TerrainId::legacy), frame, pose)?)
            } else { None };
        }
        _ => {}
    }
    Ok(after)
}

pub(super) fn configure(
    action: PlacementTransformAction,
    before: &MapPlacementRecord,
    kind: MapTerrainKind,
) -> Result<MapPlacementRecord, String> {
    if before.asset_target.is_builtin() {
        return Err("内置立方体使用盒碰撞，不能启用 GLB 三维地形或洞口".into());
    }
    if kind != MapTerrainKind::SparseClipmap {
        return Err("三维地形洞口用于 Clipmap 地图".into());
    }
    let mut after = before.clone();
    match action {
        PlacementTransformAction::ToggleTerrainPatch => {
            if after.terrain_patch.take().is_none() {
                if after.building_preset_id.is_some()
                    || after.courtyard.is_some()
                    || !after.water_surfaces.is_empty()
                {
                    return Err(
                        "建筑、院落及网格水不能同时作为三维地形，请使用独立 GLB 实例".into(),
                    );
                }
                after.terrain_patch = Some(MapPlacementTerrainPatch::default());
                after.height_mode = MapPlacementHeightMode::FixedHeight;
                after.terrain_offset = 0.0; // 保留实际世界Y，洞穴底板不随外部高度雕刻漂移。
            }
        }
        PlacementTransformAction::ToggleTerrainCutout => {
            let patch = after.terrain_patch.as_mut().ok_or("请先启用三维地形分类")?;
            patch.cutout = if patch.cutout.is_some() {
                None
            } else {
                Some(MapTerrainCutout {
                    min: [-2.0, -2.0],
                    max: [2.0, 2.0],
                })
            };
        }
        _ => return Err("无效的三维地形动作".into()),
    }
    Ok(after)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn terrain_toggle_preserves_height_and_full_record_history() {
        let before:MapPlacementRecord=serde_json::from_str(r#"{"id":"cave","asset_path":"maps/cave.glb","translation":[0,7,0],"yaw_degrees":0,"scale":[1,1,1],"height_mode":"follow_terrain","terrain_offset":2}"#).unwrap();
        let terrain = configure(
            PlacementTransformAction::ToggleTerrainPatch,
            &before,
            MapTerrainKind::SparseClipmap,
        )
        .unwrap();
        assert_eq!(terrain.translation, before.translation);
        assert!(!terrain.height_mode.follows_terrain());
        assert_eq!(terrain.terrain_offset, 0.0);
        let opened = configure(
            PlacementTransformAction::ToggleTerrainCutout,
            &terrain,
            MapTerrainKind::SparseClipmap,
        )
        .unwrap();
        assert!(opened.terrain_patch.as_ref().unwrap().cutout.is_some());
        let closed = configure(
            PlacementTransformAction::ToggleTerrainCutout,
            &opened,
            MapTerrainKind::SparseClipmap,
        )
        .unwrap();
        assert_eq!(closed.terrain_patch, terrain.terrain_patch);
        let history = EditorCommand::PlacementChanged {
            record_index: 0,
            before: before.clone(),
            after: opened.clone(),
        };
        if let EditorCommand::PlacementChanged {
            before: undo,
            after: redo,
            ..
        } = history
        {
            assert_eq!(undo, before);
            assert_eq!(redo, opened);
        } else {
            unreachable!();
        }
        assert!(
            configure(
                PlacementTransformAction::ToggleTerrainPatch,
                &before,
                MapTerrainKind::Gltf
            )
            .is_err()
        );
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn apply(
    action: PlacementTransformAction,
    target: &PlacementTransformTarget,
    before: MapPlacementRecord,
    commands: &mut Commands,
    server: &AssetServer,
    session: &mut EditorAuthoringSession,
    index: &PlacementInstanceIndex,
    roots: &Query<(Entity, &feathrise_scene::MapPlacementInstance)>,
    editor: &mut MapEditorState,
    terrain_queries: &feathrise_scene::TerrainObjectQueries,
    selected: Option<&feathrise_map_content::TerrainId>,
) -> Result<(), String> {
    let draft = session.draft.as_ref().ok_or("地图作者草稿尚未就绪")?;
    // 关闭分类后新记录已没有patch字段，仍须先证明旧源父有效，不能借换角色绕开采用门。
    crate::authoring::patch_preview_parent(draft, terrain_queries, &before)?;
    let after = configure_in_draft(action, &before, draft, selected)?;
    crate::authoring::patch_preview_parent(draft, terrain_queries, &after)?;
    let bounds = crate::authoring::placement_record_bounds(draft, &after)?;
    after.validated_transform(&draft.map_id, bounds)?; // 旋转、缩放与洞口范围在修改草稿前共同校验。
    let record = session
        .draft
        .as_mut()
        .and_then(|draft| draft.placements.placements.get_mut(target.record_index))
        .filter(|record| record.id == target.placement_id)
        .ok_or("三维地形选择已失效")?;
    *record = after.clone();
    if let Err(error) = sync_placement_record_world_with_terrain(
        commands, server, session, index, roots, &after, bounds, true, true, terrain_queries,
    ) {
        if let Some(record) = session
            .draft
            .as_mut()
            .and_then(|draft| draft.placements.placements.get_mut(target.record_index))
        {
            *record = before;
        }
        return Err(error);
    }
    session.push_bounded_water_history(EditorCommand::PlacementChanged {
        record_index: target.record_index,
        before,
        after,
    });
    session.mark_changed(editor);
    editor.status =
        "三维地形设置已更新；保存后模型、洞口、碰撞与地形拾取一起采用，Ctrl+Z可撤销".into();
    Ok(())
}
