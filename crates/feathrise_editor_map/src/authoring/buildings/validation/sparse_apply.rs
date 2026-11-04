//! 单栋稀疏整平的建筑、作者页、实时覆盖和历史事务；失败完整回滚。
use super::*;
use crate::authoring::SparseTerrainAuthoringState;
use crate::authoring::commands::BuildingSparseTerrainChange;
use feathrise_map_content::{SparseHeightLiveOverride, SparseTerrainPreparedEdit};

#[allow(clippy::too_many_arguments)]
pub(super) fn apply(
    commands: &mut Commands,
    assets: &AssetServer,
    root_index: &PlacementInstanceIndex,
    roots: &Query<(Entity, &MapPlacementInstance)>,
    editor: &mut MapEditorState,
    session: &mut EditorAuthoringSession,
    browser: &mut EditorGltfBrowserState,
    sparse: &mut SparseTerrainAuthoringState,
    live: &mut SparseHeightLiveOverride,
    result: &mut BuildingValidationResult,
) -> Result<String, String> {
    let validated = result.outcome.as_ref().map_err(Clone::clone)?;
    let request = &result.request;
    let change = match &request.operation {
        BuildingPlacementOperation::Create
        | BuildingPlacementOperation::DuplicateExisting { .. } => {
            let mut record = match &request.operation {
                BuildingPlacementOperation::DuplicateExisting { source, .. } => source.clone(),
                _ => MapPlacementRecord {
                    mesh_lod: None,
                    terrain_binding: None,
                    editor: Default::default(),
                    terrain_patch: None,
                    id: String::new(),
                    asset_path: request.preset.asset_path.clone(),
                    asset_target: request.preset.asset_target.clone(),
                    building_preset_id: Some(request.preset.id.clone()),
                    courtyard: None,
                    translation: [0.; 3],
                    height_mode: MapPlacementHeightMode::FixedHeight,
                    terrain_offset: 0.,
                    yaw_degrees: 0.,
                    scale: [1.; 3],
                    water_surfaces: Vec::new(),
                },
            };
            record.id = next_author_id(session, "building");
            record.courtyard = None;
            update_record(&mut record, request, validated);
            BuildingPlacementChange::Added {
                record_index: session
                    .draft
                    .as_ref()
                    .ok_or("地图草稿已失效")?
                    .placements
                    .placements
                    .len(),
                record,
            }
        }
        BuildingPlacementOperation::ChangeExisting {
            record_index,
            before,
            ..
        } => {
            let mut after = before.clone();
            update_record(&mut after, request, validated);
            BuildingPlacementChange::Changed {
                record_index: *record_index,
                before: before.clone(),
                after,
            }
        }
    };
    let Some(SparseBuildingLevelingState::Prepared(output)) = result.sparse_leveling.take() else {
        return Err("稀疏整平缺少完整候选".into());
    };
    let output = std::sync::Arc::try_unwrap(output).map_err(|_| "稀疏整平候选仍被其他读者共享")?;
    let changes = apply_transaction(
        commands,
        assets,
        root_index,
        roots,
        editor,
        session,
        sparse,
        live,
        &change,
        output.edit,
        false,
    )?;
    let count = changes.len();
    let command = if count == 0 {
        match &change {
            BuildingPlacementChange::Added { record, .. } => {
                EditorCommand::PlacementAdded(record.clone())
            }
            BuildingPlacementChange::Changed {
                record_index,
                before,
                after,
            } => EditorCommand::PlacementChanged {
                record_index: *record_index,
                before: before.clone(),
                after: after.clone(),
            },
        }
    } else {
        EditorCommand::BuildingSparseTerrainChanged(Box::new(BuildingSparseTerrainChange {
            placement: change.clone(),
            heights: changes,
        }))
    };
    let history = &mut *session;
    history.push_bounded_water_history(command); // 最多128笔/64MiB沿用完整线性前缀裁剪。
    if let BuildingPlacementChange::Added { record, .. } = &change {
        if matches!(
            &request.operation,
            BuildingPlacementOperation::DuplicateExisting { .. }
        ) {
            browser.selected_placement_id = Some(record.id.clone());
            browser.selection = None;
        }
    }
    Ok(format!(
        "已完成“{}”单栋整平与建筑摆放 · {count}个基础格点；可共同撤销",
        request.preset.label
    ))
}

fn update_record(
    record: &mut MapPlacementRecord,
    request: &BuildingPlacementRequest,
    validated: &ValidatedBuildingPlacement,
) {
    record.translation = validated.translation.to_array();
    record.yaw_degrees = request.yaw_degrees;
    record.scale = request.scale;
    record.height_mode = MapPlacementHeightMode::FixedHeight;
    record.terrain_offset = 0.;
    record.building_preset_id = Some(request.preset.id.clone());
    record.terrain_binding = validated.terrain_binding.clone();
}

/// 新建和历史复用同一事务。先共同证明起点，再交换有限页和记录；世界同步失败则还原。
#[allow(clippy::too_many_arguments)]
pub(in crate::authoring) fn apply_transaction(
    commands: &mut Commands,
    assets: &AssetServer,
    root_index: &PlacementInstanceIndex,
    roots: &Query<(Entity, &MapPlacementInstance)>,
    editor: &mut MapEditorState,
    session: &mut EditorAuthoringSession,
    sparse: &mut SparseTerrainAuthoringState,
    live: &mut SparseHeightLiveOverride,
    placement: &BuildingPlacementChange,
    edit: SparseTerrainPreparedEdit,
    undoing: bool,
) -> Result<std::sync::Arc<Vec<feathrise_map_content::SparseTerrainHeightChange>>, String> {
    if matches!(placement, BuildingPlacementChange::Added { .. })
        && session.placement_deletions.write_shared()
    {
        return Err("摆放删除表正在后台读取，建筑联合事务尚未执行".into());
    } // 新增和撤销新增都要换删除票；先于安装高度页，禁止共享读期间触发主线程整表复制。
    let draft = session.draft.as_ref().ok_or("稀疏联合事务地图草稿已失效")?;
    if draft.terrain_manifest().terrain.kind != feathrise_map_content::MapTerrainKind::SparseClipmap {
        return Err("联合稀疏历史不能应用到GLB地图".into());
    }
    let terrain_record = match placement { BuildingPlacementChange::Added {record,..} => record, BuildingPlacementChange::Changed {after,..} => after };
    if let Some(binding) = terrain_record.terrain_binding.as_ref() && draft.terrain_documents.active.as_ref()
        .is_none_or(|active| active.definition.id != binding.terrain_id) {
        return Err("联合建筑整平历史与当前地形作者对象不一致，拒绝写入另一块地形".into());
    }
    let bounds = draft.manifest.world_bounds().ok_or("地图范围无效")?;
    let map_id = draft.map_id.clone();
    let (index, desired, previous, structure_changed) = match placement {
        BuildingPlacementChange::Added {
            record_index,
            record,
        } => {
            if undoing {
                if *record_index + 1 != draft.placements.placements.len()
                    || draft.placements.placements.get(*record_index) != Some(record)
                {
                    return Err("联合建筑撤销尾槽或完整起点不一致".into());
                }
                (*record_index, None, Some(record.clone()), true)
            } else {
                if *record_index != draft.placements.placements.len()
                    || *record_index >= MAX_MAP_PLACEMENTS_PER_MAP
                {
                    return Err("联合建筑新增尾槽或数量预算已失效".into());
                }
                (*record_index, Some(record.clone()), None, true)
            }
        }
        BuildingPlacementChange::Changed {
            record_index,
            before,
            after,
        } => {
            let expected = if undoing { after } else { before };
            if draft.placements.placements.get(*record_index) != Some(expected) {
                return Err("联合建筑完整起点不一致，拒绝部分恢复".into());
            }
            (
                *record_index,
                Some(if undoing { before } else { after }.clone()),
                Some(expected.clone()),
                false,
            )
        }
    };
    if structure_changed && desired.is_none() {
        crate::authoring::placement_deletion::validate_deletions(session,
            previous.as_ref().map(|record| record.id.as_str()))?;
    }
    if let Some(desired) = &desired {
        desired.validated_transform(&map_id, bounds)?;
    }
    let next_water_count = draft
        .placement_water_surface_count
        .checked_sub(
            previous
                .as_ref()
                .map_or(0, |record| record.water_surfaces.len()),
        )
        .and_then(|count| {
            count.checked_add(
                desired
                    .as_ref()
                    .map_or(0, |record| record.water_surfaces.len()),
            )
        })
        .ok_or("联合建筑水绑定计数超出预算")?;
    // 私有候选已在后台验证全部页与格点。实时发布仅剩地图/修订门，提前证明后不再失败。
    if live.map_id() != Some(&map_id) || live.revision() > session.terrain_revision {
        return Err("稀疏联合事务实时高度来源已换代".into());
    }
    let adopted = sparse.install_building_edit(edit)?;
    let terrain_changed = !adopted.changes.is_empty();
    {
        let draft = session.draft.as_mut().unwrap();
        if structure_changed {
            if let Some(desired) = &desired {
                draft.placements.placements.push(desired.clone());
            } else {
                draft.placements.placements.pop();
            }
        } else {
            draft.placements.placements[index] = desired.as_ref().unwrap().clone();
        }
        draft.placement_water_surface_count = next_water_count;
    }
    if !structure_changed
        && let Some(desired) = &desired
        && let Err(error) = sync_placement_record_world(
            commands, assets, session, root_index, roots, desired, bounds, true, true,
        )
    {
        let draft = session.draft.as_mut().unwrap();
        let restored = previous.as_ref().unwrap().clone();
        draft.placement_water_surface_count = draft.placement_water_surface_count
            - desired.water_surfaces.len()
            + restored.water_surfaces.len();
        draft.placements.placements[index] = restored;
        sparse.rollback_building_edit(adopted.rollback)?;
        return Err(error);
    }
    session.mark_building_sparse_terrain_changed(editor, structure_changed, terrain_changed); // 全局、地形与结构修订只共同推进一次。
    if terrain_changed {
        live.publish_tiles(&map_id, session.terrain_revision, adopted.live_tiles)
            .expect("地图、修订和完整候选唯一页已共同预检");
    }
    if structure_changed {
        if let Some(desired) = &desired {
            crate::authoring::placement_deletion::restore_deleted(commands, session, &map_id, &desired.id);
            let entity = spawn_placement_preview(commands, assets, &map_id, desired);
            session
                .placement_previews
                .insert(desired.id.clone(), entity);
        } else if let Some(previous) = &previous {
            // 稀疏联合历史从后台返回，必须在全部采用成功后发布删除，不能绕过普通历史的保护。
            crate::authoring::placement_deletion::mark_deleted(commands, session, &map_id, &previous.id);
            if let Some(entity) = session.placement_previews.remove(&previous.id) {
                commands.entity(entity).try_despawn();
            }
        }
    }
    Ok(adopted.changes)
}

#[cfg(test)]
mod tests;
