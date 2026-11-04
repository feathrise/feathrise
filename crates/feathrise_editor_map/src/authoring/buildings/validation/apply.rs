//! 建筑后台校验结果的非阻塞接收，以及记录、预览和撤销历史的原子采用。
//!
//! 【功能设计】完成值必须再次核对地图、目录、作者修订、选择与票据，失效结果无副作用。

use super::*; // 复用父模块的身份、记录、预览同步与状态反馈协议。

/// 非阻塞接收点击结果，并在全部身份仍匹配时一次性写入地图、预览和撤销栈。
#[allow(clippy::too_many_arguments)] // 原子采用同时依赖文件目录、作者会话、UI选择与预览资产。
pub(in crate::authoring::buildings) fn poll_validation(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut editor: ResMut<MapEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut browser: ResMut<EditorGltfBrowserState>,
    root_index: Res<PlacementInstanceIndex>,
    roots: Query<(Entity, &MapPlacementInstance)>,
    registry: Option<Res<BuildingPresetRegistry>>,
    controls: Res<BuildingPlacementControls>,
    clipmap: Option<feathrise_scene::TerrainObjectClipmapGround>,
    mut state: ResMut<BuildingAuthoringState>,
    mut building_ui: ResMut<BuildingPresetEditorState>,
    mut sparse: Option<ResMut<crate::authoring::SparseTerrainAuthoringState>>,
    mut live: Option<ResMut<feathrise_map_content::SparseHeightLiveOverride>>,
) {
    if let Some(task) = state.validation_task.as_mut() {
        let Some(output) = check_ready(task) else {
            return;
        };
        state.validation_task = None;
        if let Some(lease) = output.lease
            && let Some(sparse) = sparse.as_deref_mut()
        {
            sparse.return_building_author(lease);
        }
        state.validation_waiting = Some(output.result);
    }
    let Some(mut result) = state.validation_waiting.take() else {
        return;
    };
    let Some(registry) = registry.as_deref() else {
        report(
            &mut editor,
            &mut building_ui,
            "建筑校验完成时预设目录已卸载；结果已丢弃",
        );
        return;
    };
    let current = current_identity(&session, registry);
    let operation_valid = editor.open
        && !editor.close_requested
        && !editor.new_map_dialog_open
        && !session.saving
        && editor.selected_map_id.as_ref() == Some(&result.request.identity.map_id)
        && result
            .request
            .input_controls
            .is_none_or(|identity| super::super::ground::controls_identity(&controls) == identity)
        && match &result.request.operation {
            BuildingPlacementOperation::Create => {
                let selected_matches = editor
                    .building_selected_preset_id
                    .as_deref()
                    .and_then(|id| registry.resolve(id))
                    .is_some_and(|preset| preset.id == result.request.preset.id);
                editor.tool == EditorTool::Building
                    && selected_matches
                    && result.request.input_controls.is_none_or(|identity| {
                        super::super::ground::controls_identity(&controls) == identity
                    })
            }
            BuildingPlacementOperation::ChangeExisting {
                record_index,
                before,
                ..
            }
            | BuildingPlacementOperation::DuplicateExisting {
                record_index,
                source: before,
            } => {
                browser.selected_placement_id.as_deref() == Some(before.id.as_str())
                    && session.draft.as_ref().is_some_and(|draft| {
                        draft
                            .placements
                            .placements
                            .get(*record_index)
                            .is_some_and(|record| record.id == before.id)
                    })
            }
        };
    if current.as_ref() == Some(&result.request.identity)
        && operation_valid
        && let Some(draft) = session.draft.as_ref()
    {
        match super::super::ground::content_is_current(
            result.request.ground_content,
            draft,
            clipmap.as_deref(),
            session.terrain_revision,
        ) {
            Ok(false) => {
                state.validation_waiting = Some(result);
                return;
            }
            Err(error) => {
                report(&mut editor, &mut building_ui, &error);
                return;
            }
            Ok(true) => {}
        }
    }
    let identity_valid = current.as_ref() == Some(&result.request.identity)
        && state.active_identity.as_ref() == Some(&result.request.identity)
        && result.request.ticket == state.next_ticket
        && operation_valid;
    if !identity_valid {
        report(
            &mut editor,
            &mut building_ui,
            "建筑校验结果已过期；地图、选择、预设或作者修订发生了变化，请重新操作",
        );
        return; // 迟到结果不得创建 ECS、作者记录或撤销命令。
    }
    if result.request.level_terrain && session.draft.as_ref().is_none_or(|draft|
        !super::super::terrain_coordinates::matches(draft, result.request.terrain_frame.as_ref())) {
        state.last_validation = Some(result);
        report(&mut editor, &mut building_ui, "建筑整平的作者对象已经变化；已保留本次落点，请在承托对象上重新确认");
        return;
    }
    if let Some(query) = result.ground_query.as_ref() {
        let draft = session.draft.as_ref().unwrap();
        let Ok(sampler) = PlacementTerrainSampler::from_sources(
            draft,
            clipmap.as_deref(),
            session.terrain_revision,
        ) else {
            state.validation_waiting = Some(result);
            return;
        };
        match query.can_retry(&sampler) {
            Ok(false) => {
                state.validation_waiting = Some(result);
                return;
            }
            Err(error) => {
                report(&mut editor, &mut building_ui, &error);
                return;
            }
            Ok(true) => {}
        }
        let query = result.ground_query.take();
        let index = state.active_index.as_ref().unwrap().clone();
        let bounds = draft.manifest.world_bounds().unwrap();
        let request = result.request;
        state.validation_task = Some(AsyncComputeTaskPool::get().spawn(async move {
            validate_with_ground(request, &index, &sampler, bounds, query).into()
        }));
        return; // 等待只占原点击槽；主线程不遍历五点或IO。
    }
    if matches!(
        &result.sparse_leveling,
        Some(SparseBuildingLevelingState::WaitingForAuthor)
    ) {
        let Some(lease) = sparse
            .as_deref_mut()
            .and_then(|state| state.take_building_author())
        else {
            state.validation_waiting = Some(result);
            return;
        };
        let index = state.active_index.as_ref().unwrap().clone();
        let bounds = session
            .draft
            .as_ref()
            .unwrap()
            .manifest
            .world_bounds()
            .unwrap();
        let snapshot = clipmap
            .as_deref()
            .and_then(|surface| surface.snapshot.as_ref())
            .unwrap()
            .clone();
        state.validation_task = Some(super::sparse_leveling::start(
            result, index, bounds, lease, snapshot,
        ));
        return;
    }
    if let Err(error) = &result.outcome {
        let message = format!("建筑位置无效：{error}");
        state.last_validation = Some(result); // 保留红色占地和门区辅助线。
        report(&mut editor, &mut building_ui, &message);
        return;
    }
    if session.saving {
        report(
            &mut editor,
            &mut building_ui,
            "地图已开始保存，本次建筑落点已安全取消",
        );
        return;
    }
    let adds_record = matches!(
        &result.request.operation,
        BuildingPlacementOperation::Create | BuildingPlacementOperation::DuplicateExisting { .. }
    );
    if adds_record
        && session
            .draft
            .as_ref()
            .is_some_and(|draft| draft.placements.placements.len() >= MAX_MAP_PLACEMENTS_PER_MAP)
    {
        report(
            &mut editor,
            &mut building_ui,
            &format!("当前地图已达到{MAX_MAP_PLACEMENTS_PER_MAP}个显式摆放上限"),
        );
        return; // 后台期间其他入口可能刚好填满容量。
    }
    if matches!(
        &result.sparse_leveling,
        Some(SparseBuildingLevelingState::Prepared(_))
    ) {
        if adds_record && session.placement_deletions.write_shared() {
            state.validation_waiting = Some(result);
            editor.status = "摆放删除表正在后台准备；完成后继续采用建筑整平结果".into();
            return; // 完整页候选仍归原槽，不消耗落点，也不先改建筑、地形或历史。
        }
        let applied = sparse
            .as_deref_mut()
            .zip(live.as_deref_mut())
            .ok_or("稀疏作者状态或实时覆盖未就绪".to_owned())
            .and_then(|(sparse, live)| {
                super::sparse_apply::apply(
                    &mut commands,
                    &asset_server,
                    &root_index,
                    &roots,
                    &mut editor,
                    &mut session,
                    &mut browser,
                    sparse,
                    live,
                    &mut result,
                )
            });
        let message = applied.unwrap_or_else(|error| format!("建筑稀疏整平采用失败：{error}"));
        state.last_validation = Some(result);
        report(&mut editor, &mut building_ui, &message);
        return;
    }
    let validated = result
        .outcome
        .as_ref()
        .expect("上方已经处理非法结果")
        .clone();
    let operation = result.request.operation.clone();
    let terrain_leveling = result.terrain_leveling.take(); // 大型新网格只移入采用函数，不保留在红绿回显状态。
    let apply_result = match operation {
        BuildingPlacementOperation::Create => apply_created_building(
            &mut commands,
            &asset_server,
            &mut editor,
            &mut session,
            &result.request,
            &validated,
            terrain_leveling,
        ),
        BuildingPlacementOperation::ChangeExisting {
            record_index,
            before,
            kind,
        } => apply_changed_building(
            &mut commands,
            &asset_server,
            &root_index,
            &roots,
            &mut editor,
            &mut session,
            record_index,
            before,
            kind,
            &result.request,
            &validated,
            terrain_leveling,
        ),
        BuildingPlacementOperation::DuplicateExisting { source, .. } => apply_duplicated_building(
            &mut commands,
            &asset_server,
            &mut browser,
            &mut editor,
            &mut session,
            source,
            &result.request,
            &validated,
            terrain_leveling,
        ),
    };
    let message = match apply_result {
        Ok(message) => message,
        Err(error) => {
            report(
                &mut editor,
                &mut building_ui,
                &format!("建筑操作采用失败：{error}"),
            );
            return;
        }
    };
    state.last_validation = Some(result); // 下一帧因作者修订变化自动失效，不冒充新索引。
    report(&mut editor, &mut building_ui, &message);
}

/// 新建建筑在校验成功后一次提交记录、预览和撤销历史。
fn apply_created_building(
    commands: &mut Commands,
    asset_server: &AssetServer,
    editor: &mut MapEditorState,
    session: &mut EditorAuthoringSession,
    request: &BuildingPlacementRequest,
    validated: &ValidatedBuildingPlacement,
    terrain_leveling: Option<TerrainLevelingOutput>,
) -> Result<String, String> {
    let map_id = request.identity.map_id.clone();
    let id = next_author_id(session, "building");
    let record = MapPlacementRecord {
        mesh_lod: None,
        terrain_binding: validated.terrain_binding.clone(),
        editor: Default::default(),
        terrain_patch: None,
        id: id.clone(),
        asset_path: request.preset.asset_path.clone(),
        asset_target: request.preset.asset_target.clone(),
        building_preset_id: Some(request.preset.id.clone()),
        courtyard: None, // 独立摆放记录不属于任何院落副本。
        translation: validated.translation.to_array(),
        height_mode: MapPlacementHeightMode::FixedHeight,
        terrain_offset: 0.0,
        yaw_degrees: request.yaw_degrees,
        scale: request.scale,
        water_surfaces: Vec::new(),
    };
    let record_index = session
        .draft
        .as_ref()
        .map(|draft| draft.placements.placements.len())
        .ok_or_else(|| "地图作者草稿已经失效".to_owned())?;
    let installed = install_terrain_leveling(session, request, terrain_leveling)?; // 地形Arc只在全部身份预检后换代。
    let draft = session.draft.as_mut().expect("上方已确认作者草稿存在");
    draft.placements.placements.push(record.clone());
    let entity = spawn_placement_preview(commands, asset_server, &map_id, &record); // 后续没有可失败分支才登记延迟实体。
    session.placement_previews.insert(id, entity);
    let leveled_vertices = installed.as_ref().map_or(0, |value| value.changes.len());
    let command = installed.map_or_else(
        || EditorCommand::PlacementAdded(record.clone()),
        |value| {
            EditorCommand::BuildingTerrainChanged(Box::new(BuildingTerrainChange {
                placement: BuildingPlacementChange::Added {
                    record_index,
                    record: record.clone(),
                },
                heights: value.changes,
            }))
        },
    );
    push_history(session, command); // 建筑与地形只有一条线性撤销记录。
    if leveled_vertices > 0 {
        session.mark_building_terrain_changed(editor, true, true); // 结构与地形专用修订同次发布。
    } else {
        session.mark_placement_structure_changed(editor);
    }
    editor.building_selected_preset_id = Some(request.preset.id.clone());
    Ok(format!(
        "已放置“{}” · 地面高差 {:.3} 米{}；可撤销",
        request.preset.label,
        validated.maximum_ground - validated.minimum_ground,
        leveling_suffix(leveled_vertices),
    ))
}

/// 已有建筑移动或九十度旋转成功后原子替换同一记录并同步可见根。
#[allow(clippy::too_many_arguments)]
fn apply_changed_building(
    commands: &mut Commands,
    asset_server: &AssetServer,
    root_index: &PlacementInstanceIndex,
    roots: &Query<(Entity, &MapPlacementInstance)>,
    editor: &mut MapEditorState,
    session: &mut EditorAuthoringSession,
    record_index: usize,
    before: MapPlacementRecord,
    kind: BuildingChangeKind,
    request: &BuildingPlacementRequest,
    validated: &ValidatedBuildingPlacement,
    terrain_leveling: Option<TerrainLevelingOutput>,
) -> Result<String, String> {
    let bounds = session
        .draft
        .as_ref()
        .and_then(|draft| draft.manifest.world_bounds())
        .ok_or_else(|| "当前地图世界范围无效".to_owned())?;
    let mut after = before.clone();
    after.translation = validated.translation.to_array();
    after.height_mode = MapPlacementHeightMode::FixedHeight;
    after.terrain_offset = 0.0;
    after.yaw_degrees = request.yaw_degrees;
    after.scale = request.scale;
    after.terrain_binding = validated.terrain_binding.clone();
    after.building_preset_id = Some(request.preset.id.clone());
    session
        .draft
        .as_ref()
        .and_then(|draft| draft.placements.placements.get(record_index))
        .filter(|record| *record == &before)
        .ok_or_else(|| "建筑记录在联合采用前已经失效".to_owned())?; // 地形换指针前先证明建筑侧完整起点仍一致。
    let installed = install_terrain_leveling(session, request, terrain_leveling)?; // 保存旧Arc供世界同步失败时完整回滚。
    let current = session
        .draft
        .as_mut()
        .and_then(|draft| draft.placements.placements.get_mut(record_index))
        .filter(|record| record.id == before.id)
        .expect("上方已在地形采用前预检建筑记录完整起点");
    *current = after.clone();
    if let Err(error) = sync_placement_record_world(
        commands,
        asset_server,
        session,
        root_index,
        roots,
        &after,
        bounds,
        true,
        true,
    ) {
        if let Some(current) = session
            .draft
            .as_mut()
            .and_then(|draft| draft.placements.placements.get_mut(record_index))
            .filter(|record| record.id == before.id)
        {
            *current = before;
        }
        rollback_terrain_leveling(session, installed.as_ref()); // 建筑画面同步失败时地形与记录共同恢复。
        return Err(format!("同步建筑预览失败：{error}"));
    }
    let leveled_vertices = installed.as_ref().map_or(0, |value| value.changes.len());
    let command = installed.map_or_else(
        || EditorCommand::PlacementChanged {
            record_index,
            before: before.clone(),
            after: after.clone(),
        },
        |value| {
            EditorCommand::BuildingTerrainChanged(Box::new(BuildingTerrainChange {
                placement: BuildingPlacementChange::Changed {
                    record_index,
                    before: before.clone(),
                    after: after.clone(),
                },
                heights: value.changes,
            }))
        },
    );
    push_history(session, command);
    if leveled_vertices > 0 {
        session.mark_building_terrain_changed(editor, false, true); // 移动不改数组结构，只换代地形与统一作者修订。
    } else {
        session.mark_changed(editor);
    }
    let action = match kind {
        BuildingChangeKind::Relocate => "已移动并重新解析地基",
        BuildingChangeKind::RotateQuarterTurn => "已顺时针旋转90°并重新校验",
        BuildingChangeKind::RotateSnapped => "已按90°档位旋转并重新校验",
    };
    Ok(format!(
        "{action}：{} · 地面高差 {:.3} 米{}；可撤销",
        after.id,
        validated.maximum_ground - validated.minimum_ground,
        leveling_suffix(leveled_vertices),
    ))
}

/// 建筑复制在合法落点确认前不生成任何重叠作者记录或Scene0根。
#[allow(clippy::too_many_arguments)]
fn apply_duplicated_building(
    commands: &mut Commands,
    asset_server: &AssetServer,
    browser: &mut EditorGltfBrowserState,
    editor: &mut MapEditorState,
    session: &mut EditorAuthoringSession,
    source: MapPlacementRecord,
    request: &BuildingPlacementRequest,
    validated: &ValidatedBuildingPlacement,
    terrain_leveling: Option<TerrainLevelingOutput>,
) -> Result<String, String> {
    let draft = session
        .draft
        .as_ref()
        .ok_or_else(|| "地图作者草稿已经失效".to_owned())?;
    if draft.placements.placements.len() >= MAX_MAP_PLACEMENTS_PER_MAP {
        return Err(format!(
            "当前地图已达到{MAX_MAP_PLACEMENTS_PER_MAP}个显式摆放上限"
        ));
    }
    let next_surface_count = draft
        .placement_water_surface_count
        .checked_add(source.water_surfaces.len())
        .ok_or_else(|| "整网格水绑定计数溢出".to_owned())?;
    let map_id = draft.map_id.clone();
    let record_index = draft.placements.placements.len(); // 新副本只能追加到当前冻结尾槽。
    let mut duplicate = source;
    duplicate.id = next_author_id(session, "building");
    duplicate.courtyard = None; // 单栋复制是独立建筑，不继承原院落副本的成员身份。
    duplicate.translation = validated.translation.to_array();
    duplicate.height_mode = MapPlacementHeightMode::FixedHeight;
    duplicate.terrain_offset = 0.0;
    duplicate.yaw_degrees = request.yaw_degrees;
    duplicate.scale = request.scale;
    duplicate.terrain_binding = validated.terrain_binding.clone();
    duplicate.building_preset_id = Some(request.preset.id.clone());
    let installed = install_terrain_leveling(session, request, terrain_leveling)?;
    let entity = spawn_placement_preview(commands, asset_server, &map_id, &duplicate); // 全部可失败预检之后才登记实体。
    let draft = session
        .draft
        .as_mut()
        .expect("上方已在联合采用前确认地图作者草稿存在");
    draft.placement_water_surface_count = next_surface_count;
    draft.placements.placements.push(duplicate.clone());
    session
        .placement_previews
        .insert(duplicate.id.clone(), entity);
    let leveled_vertices = installed.as_ref().map_or(0, |value| value.changes.len());
    let command = installed.map_or_else(
        || EditorCommand::PlacementAdded(duplicate.clone()),
        |value| {
            EditorCommand::BuildingTerrainChanged(Box::new(BuildingTerrainChange {
                placement: BuildingPlacementChange::Added {
                    record_index,
                    record: duplicate.clone(),
                },
                heights: value.changes,
            }))
        },
    );
    push_history(session, command);
    if leveled_vertices > 0 {
        session.mark_building_terrain_changed(editor, true, true);
    } else {
        session.mark_placement_structure_changed(editor);
    }
    browser.selected_placement_id = Some(duplicate.id.clone());
    browser.selection = None;
    Ok(format!(
        "已在合法落点复制建筑 {} · 地面高差 {:.3} 米{}；可撤销",
        duplicate.id,
        validated.maximum_ground - validated.minimum_ground,
        leveling_suffix(leveled_vertices),
    ))
}

/// 主线程采用的临时回滚信息；历史只保留局部差分，不长期持有整份旧GLB。
struct InstalledTerrainLeveling {
    previous: std::sync::Arc<feathrise_map_terrain::TerrainMeshInput>, // 世界同步失败时O(1)恢复旧作者源。
    changes: Vec<crate::authoring::HeightChange>,                // 成功后移入唯一复合历史命令。
}

/// 在当前GLB身份和全部局部差分仍匹配时一次换入后台整平网格。
fn install_terrain_leveling(
    session: &mut EditorAuthoringSession,
    request: &BuildingPlacementRequest,
    output: Option<TerrainLevelingOutput>,
) -> Result<Option<InstalledTerrainLeveling>, String> {
    if !request.level_terrain {
        if output.is_some() {
            return Err("普通建筑请求意外携带地形整平结果".to_owned());
        }
        return Ok(None); // 默认路径完全不接触地形Arc。
    }
    let output = output.ok_or_else(|| "联合整平成功结果缺少完整地形候选".to_owned())?;
    let draft = session
        .draft
        .as_mut()
        .ok_or_else(|| "联合整平采用时地图草稿已经失效".to_owned())?;
    let current = draft
        .terrain_mesh
        .as_ref()
        .ok_or_else(|| "联合整平采用时GLB地形已经失效".to_owned())?;
    if std::sync::Arc::as_ptr(current) as usize != output.source_mesh_identity {
        return Err("联合整平采用时地形作者源已经换代".to_owned());
    }
    if current.positions.len() != output.mesh.positions.len()
        || current.indices.len() != output.mesh.indices.len()
        || output.changes.iter().any(|change| {
            change.index >= current.positions.len()
                || current.positions[change.index][1] != change.before
                || output.mesh.positions[change.index][1] != change.after
                || change.after < change.before
        })
    {
        return Err("联合整平候选与当前GLB顶点差分不一致".to_owned());
    }
    if output.changes.is_empty() {
        return Ok(None); // 空差分仍先核对源Arc与拓扑，随后只提交建筑侧命令和修订。
    }
    let previous = current.clone(); // 只为当前采用调用保留，成功后立即释放。
    draft.terrain_mesh = Some(output.mesh); // 完整候选在主线程以一次Arc换指针原子采用。
    Ok(Some(InstalledTerrainLeveling {
        previous,
        changes: output.changes,
    }))
}

/// 建筑世界同步失败时把刚采用的地形Arc恢复到事务前版本。
fn rollback_terrain_leveling(
    session: &mut EditorAuthoringSession,
    installed: Option<&InstalledTerrainLeveling>,
) {
    if let Some(installed) = installed
        && let Some(draft) = session.draft.as_mut()
    {
        draft.terrain_mesh = Some(installed.previous.clone()); // 历史和修订尚未发布，回滚无额外副作用。
    }
}

/// 将命令加入统一线性历史；含网格水记录时继续服从既有内存硬门。
fn push_history(session: &mut EditorAuthoringSession, command: EditorCommand) {
    if command_uses_bounded_water_history(&command) {
        let history = &mut *session;
        history.push_bounded_water_history(command);
    } else {
        session.redo.clear(); // 新作者分支无论是否含水都必须终止旧重做链。
        session.undo.push(command);
    }
}

/// 只在真实地形顶点发生变化时追加明确的联合整平反馈。
fn leveling_suffix(changed_vertices: usize) -> String {
    if changed_vertices == 0 {
        String::new() // 已平坦平台不伪称修改地形。
    } else {
        format!(" · 联合整平 {changed_vertices} 个地形顶点")
    }
}

#[cfg(test)]
mod tests {
    use super::*; // 采用门测试直接访问私有Arc身份与回滚帮助函数。
    use feathrise_map_content::{BuildingPresetDescriptor, MapPlacementAssetTarget};
    use std::sync::Arc;

    /// 构造只用于地形采用门的轻量建筑请求。
    fn leveling_request(session: &EditorAuthoringSession) -> BuildingPlacementRequest {
        let draft = session.draft.as_ref().expect("测试草稿存在");
        BuildingPlacementRequest {
            terrain_frame: None,
            identity: BuildingIndexIdentity {
                map_id: draft.map_id.clone(),
                author_revision: session.revision,
                terrain_revision: session.terrain_revision,
                structure_revision: session.placement_structure_revision,
                catalog_revision: 1,
            },
            ticket: 1,
            preset: BuildingPresetDescriptor::from_asset(
                "level-test".to_owned(),
                "整平测试".to_owned(),
                "gltf/test.glb".to_owned(),
                MapPlacementAssetTarget::Scene0,
            ),
            world_xz: Vec2::ZERO,
            yaw_degrees: 0.0,
            scale: [1.0; 3],
            exclude_id: None,
            operation: BuildingPlacementOperation::Create,
            level_terrain: true,
            terrain_leveling: None,
            ground_content: None,
            input_controls: None,
        }
    }

    #[test]
    fn clipmap_ready_result_waits_for_surface_and_author_selection_or_close_cancels_without_writes()
    {
        use feathrise_map_content::{MapTerrainKind, ResolvedBuildingPresetCatalog};
        for invalidation in 0..4 {
            let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
            draft.manifest.terrain.kind = MapTerrainKind::SparseClipmap;
            let source = MapPlacementRecord {
                mesh_lod: None,
                terrain_binding: None,
                editor: Default::default(),
                terrain_patch: None,
                id: "house-1".into(),
                asset_path: "gltf/test.glb".into(),
                asset_target: Default::default(),
                building_preset_id: Some("house".into()),
                courtyard: None,
                translation: [5., 2., 5.],
                height_mode: MapPlacementHeightMode::FixedHeight,
                terrain_offset: 0.,
                yaw_degrees: 0.,
                scale: [1.; 3],
                water_surfaces: Vec::new(),
            };
            draft.placements.placements.push(source.clone());
            let map_id = draft.map_id.clone();
            let session = EditorAuthoringSession {
                draft: Some(draft),
                ..default()
            };
            let mut request = leveling_request(&session);
            request.level_terrain = false;
            request.ground_content = Some([7; 7]);
            request.operation = BuildingPlacementOperation::ChangeExisting {
                record_index: 0,
                before: source.clone(),
                kind: BuildingChangeKind::Relocate,
            };
            let footprint = request.preset.world_footprint([5.; 3], 0., [1.; 3]);
            let state = BuildingAuthoringState {
                active_identity: Some(request.identity.clone()),
                active_index: Some(Arc::new(BuildingSpatialIndex::default())),
                next_ticket: request.ticket,
                validation_waiting: Some(BuildingValidationResult {
                    request,
                    footprint,
                    entrance_clearance: footprint,
                    outcome: Ok(ValidatedBuildingPlacement {
                        terrain_binding: None,
                        translation: Vec3::splat(99.),
                        minimum_ground: 99.,
                        maximum_ground: 99.,
                    }),
                    terrain_leveling: None,
                    ground_query: None,
                    sparse_leveling: None,
                }),
                ..default()
            };
            let mut app = App::new();
            app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
                .insert_resource(session)
                .insert_resource(state)
                .insert_resource(MapEditorState {
                    open: true,
                    selected_map_id: Some(map_id),
                    ..default()
                })
                .insert_resource(BuildingPresetRegistry {
                    catalog: default(),
                    revision: 1,
                    resolved: ResolvedBuildingPresetCatalog::default(),
                    last_error: None,
                })
                .init_resource::<BuildingPlacementControls>()
                .init_resource::<BuildingPresetEditorState>()
                .init_resource::<EditorGltfBrowserState>()
                .init_resource::<PlacementInstanceIndex>()
                .init_resource::<DynamicClipmapGroundSurface>()
                .add_systems(Update, poll_validation);
            app.world_mut()
                .resource_mut::<EditorGltfBrowserState>()
                .selected_placement_id = Some(source.id.clone());
            for _ in 0..3 {
                app.update();
                assert!(
                    app.world()
                        .resource::<BuildingAuthoringState>()
                        .validation_waiting
                        .is_some()
                );
            }
            match invalidation {
                0 => {
                    app.world_mut()
                        .resource_mut::<EditorAuthoringSession>()
                        .terrain_revision += 1;
                }
                1 => {
                    app.world_mut()
                        .resource_mut::<EditorGltfBrowserState>()
                        .selected_placement_id = Some("other".into());
                }
                2 => {
                    app.world_mut()
                        .resource_mut::<MapEditorState>()
                        .close_requested = true;
                }
                _ => {
                    app.world_mut()
                        .resource_mut::<BuildingPresetRegistry>()
                        .revision += 1;
                }
            }
            app.update();
            assert!(
                app.world()
                    .resource::<BuildingAuthoringState>()
                    .validation_waiting
                    .is_none()
            );
            let session = app.world().resource::<EditorAuthoringSession>();
            assert_eq!(
                session.draft.as_ref().unwrap().placements.placements,
                vec![source]
            );
            assert!(session.undo.is_empty());
            assert!(!app.world().resource::<MapEditorState>().dirty);
        }
    }

    #[test]
    fn stale_leveling_source_is_rejected_without_replacing_current_terrain() {
        let draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
        let mut session = EditorAuthoringSession {
            draft: Some(draft),
            terrain_revision: 3,
            ..default()
        };
        let request = leveling_request(&session);
        let current = session
            .draft
            .as_ref()
            .unwrap()
            .terrain_mesh
            .as_ref()
            .unwrap()
            .clone();
        let mut candidate = (*current).clone();
        candidate.positions[0][1] += 1.0;
        let output = TerrainLevelingOutput {
            source_mesh_identity: (Arc::as_ptr(&current) as usize).wrapping_add(1), // 故意伪造上一地形Arc身份。
            mesh: Arc::new(candidate),
            changes: vec![crate::authoring::HeightChange {
                index: 0,
                before: current.positions[0][1],
                after: current.positions[0][1] + 1.0,
            }],
            platform_height: current.positions[0][1] + 1.0,
        };

        assert!(install_terrain_leveling(&mut session, &request, Some(output)).is_err());
        let after = session
            .draft
            .as_ref()
            .unwrap()
            .terrain_mesh
            .as_ref()
            .unwrap();
        assert!(Arc::ptr_eq(after, &current)); // 迟到任务没有留下任何地形写入。
    }

    #[test]
    fn installed_leveling_can_rollback_by_restoring_the_previous_arc() {
        let draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
        let mut session = EditorAuthoringSession {
            draft: Some(draft),
            terrain_revision: 4,
            ..default()
        };
        let request = leveling_request(&session);
        let current = session
            .draft
            .as_ref()
            .unwrap()
            .terrain_mesh
            .as_ref()
            .unwrap()
            .clone();
        let before = current.positions[0][1];
        let mut candidate = (*current).clone();
        candidate.positions[0][1] = before + 1.0;
        let output = TerrainLevelingOutput {
            source_mesh_identity: Arc::as_ptr(&current) as usize,
            mesh: Arc::new(candidate),
            changes: vec![crate::authoring::HeightChange {
                index: 0,
                before,
                after: before + 1.0,
            }],
            platform_height: before + 1.0,
        };

        let installed = install_terrain_leveling(&mut session, &request, Some(output))
            .expect("同源候选应可采用")
            .expect("真实差分应换入新地形");
        assert!(!Arc::ptr_eq(
            session
                .draft
                .as_ref()
                .unwrap()
                .terrain_mesh
                .as_ref()
                .unwrap(),
            &current,
        ));
        rollback_terrain_leveling(&mut session, Some(&installed));
        assert!(Arc::ptr_eq(
            session
                .draft
                .as_ref()
                .unwrap()
                .terrain_mesh
                .as_ref()
                .unwrap(),
            &current,
        )); // 后续建筑同步失败时旧建筑与旧地形可共同恢复。
    }

    #[test]
    fn stale_noop_leveling_source_is_rejected_before_building_adoption() {
        let draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121])); // 使用具有真实GLB源的测试草稿。
        let mut session = EditorAuthoringSession {
            draft: Some(draft),  // 当前会话持有权威地形Arc。
            terrain_revision: 5, // 空差分也属于明确地形修订。
            ..default()          // 其他作者状态不影响采用门。
        };
        let request = leveling_request(&session); // 请求冻结当前会话身份。
        let current = session
            .draft
            .as_ref()
            .unwrap()
            .terrain_mesh
            .as_ref()
            .unwrap()
            .clone(); // 输出网格可以浅共享当前值，但源身份故意伪造。
        let output = TerrainLevelingOutput {
            source_mesh_identity: (Arc::as_ptr(&current) as usize).wrapping_add(1), // 模拟同修订异常换源后的迟到空结果。
            mesh: current.clone(),                    // 空差分无需构造新网格。
            changes: Vec::new(),                      // 平台原本已满足高度。
            platform_height: current.positions[0][1], // 建筑根曾依据旧结果解析的高度。
        };

        assert!(install_terrain_leveling(&mut session, &request, Some(output)).is_err()); // 无顶点变化也必须拒绝旧源。
        assert!(Arc::ptr_eq(
            session
                .draft
                .as_ref()
                .unwrap()
                .terrain_mesh
                .as_ref()
                .unwrap(),
            &current,
        )); // 失败采用没有替换当前作者地形。
    }
}
