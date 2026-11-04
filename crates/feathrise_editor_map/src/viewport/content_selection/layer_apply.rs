//! 跨类型候选的唯一正式选层出口；悬停仅发布临时覆盖身份，不更改作者数据。

use super::layers::{LayerCandidateState, LayerTarget};

pub(crate) fn sync_sidebar_layer_selection(
    editor: Res<MapEditorState>,
    session: Res<EditorAuthoringSession>,
    domain: Res<MapInteractionState>,
    modes: Res<ViewportModeState>,
    mut state: ResMut<LayerCandidateState>,
    sidebars: Res<MapEditorSidebarState>, // 异步新建角色层仍属于当前角色侧栏。
    browser: Res<EditorGltfBrowserState>, // 场景树明确选中实例也应接管统一高亮。
    boundary: Res<BoundaryEditState>,     // 玩法侧栏已有路径选择只读同步。
    mut observed_placement: Local<(
        Option<String>,
        Option<crate::ui::gltf_browser::EditorGltfMeshSelection>,
    )>, // 同实例切换网格也属于明确场景树选择。
    mut observed_domain_layer: Local<Option<String>>, // 只观察真实选层变化，不把悬停当选择。
) {
    let placement_changed = observed_placement.0 != browser.selected_placement_id
        || observed_placement.1 != browser.selection; // 只比较有界的稳定网格路径。
    if placement_changed {
        *observed_placement = (
            browser.selected_placement_id.clone(),
            browser.selection.clone(),
        ); // 记录包括同实例换网格在内的选择边沿。
        let same_candidate_mesh = browser.selected_placement_id.as_ref().is_some_and(|id| {
            state.selected.as_ref() == Some(&LayerTarget::Placement(id.clone()))
                && state.placement_mesh_selection(id) == browser.selection.as_ref()
        }); // 统一候选刚应用时不反向清空列表。
        if editor.open
            && modes.selection_allowed(&editor)
            && let Some(id) = browser.selected_placement_id.as_ref()
            && !same_candidate_mesh
            && let Some(draft) = session.draft.as_ref()
        {
            state.clear(); // 场景树手动选中后取消旧作者候选和迟到任务。
            state.identity = Some(super::layers::Identity {
                map: draft.map_id.clone(),  // 场景树选择同样不可跨地图继承。
                revision: session.revision, // 作者修改后由统一轮询淘汰旧身份。
                placement_derived_pose_revision: session.placement_derived_pose_revision,
                domain_hash: draft.adopted_domain_hash().to_owned(), // 保持与所有候选一致的完整修订门。
                generation: domain.observed_generation,          // 同图重进也清理旧选择。
                ticket: state.ticket,                                // 使用clear之后的新票据。
                filter: modes.selection_filter(&editor), // 场景树与视口使用同一用户筛选身份。
            });
            state.selected = Some(LayerTarget::Placement(id.clone())); // 保持所有类型只有一个正式目标。
            if let Some(selection) = browser.selection.as_ref() {
                state.selected_placement_hit = Some(super::placement::PlacementViewportHit::SceneMesh(selection.clone()));
                state.placement_hits.insert(
                    id.clone(),
                    super::placement::PlacementViewportHit::SceneMesh(selection.clone()),
                ); // 侧栏精确网格仍可供统一覆盖读取。
            }
            return; // 作者工具记忆不能覆盖本次场景树选择。
        }
    }
    // 侧栏先提交明确选择，再由模式同步恢复Inspect。
    let domain_changed = *observed_domain_layer != domain.selected_layer; // 完整后台新层采用后变化。
    if domain_changed {
        *observed_domain_layer = domain.selected_layer.clone();
    } // 稳定帧不复制身份。
    let asynchronous_layer = domain_changed
        && matches!(sidebars.left, EditorLeftSidebarTab::Domain(_))
        && domain
            .selected_layer
            .as_ref()
            .is_some_and(|id| state.selected.as_ref() != Some(&LayerTarget::Domain(id.clone()))); // 地图候选刚确认时不清空其菜单。
    if !editor.open
        || !modes.selection_allowed(&editor)
        || (editor.tool == EditorTool::Inspect && !asynchronous_layer)
    {
        return;
    } // 稳定帧不反复覆盖视口候选。
    let Some(draft) = session.draft.as_ref() else {
        return;
    }; // 无有效地图不创建选择。
    let selected = match if asynchronous_layer {
        EditorTool::Domain(domain.domain())
    } else {
        editor.tool
    } {
        // 每类仅查询一个已选索引。
        EditorTool::Portal => editor.selected_current_portal_id.as_ref().and_then(|id| {
            draft
                .portals
                .portals
                .iter()
                .position(|portal| portal.id == *id)
                .map(|index| (LayerTarget::Portal(id.clone()), Some(index)))
        }), // 显式侧栏边沿才定位作者索引。
        EditorTool::Boundary => boundary.path_index.and_then(|index| {
            draft
                .boundaries
                .as_ref()?
                .paths
                .get(index)
                .map(|path| (LayerTarget::Boundary(path.id.clone()), Some(index)))
        }), // 不把控制点拖动当作新点击。
        EditorTool::Surface => draft
            .surface
            .as_ref()
            .and_then(|surface| surface.layers.get(editor.selected_surface_layer))
            .filter(|layer| {
                editor
                    .surface_selected_preset_id
                    .as_ref()
                    .is_none_or(|id| *id == layer.preset_id)
            })
            .map(|layer| {
                (
                    LayerTarget::Surface(layer.id.clone()),
                    Some(editor.selected_surface_layer),
                )
            }), // 新预设未归层时不误选旧索引。
        EditorTool::Vegetation => draft
            .vegetation
            .as_ref()
            .and_then(|vegetation| vegetation.authoring.as_ref())
            .and_then(|authoring| authoring.layers.get(editor.selected_vegetation_layer))
            .filter(|layer| layer.enabled)
            .map(|layer| {
                (
                    LayerTarget::Vegetation(layer.id.clone()),
                    Some(editor.selected_vegetation_layer),
                )
            }), // 隐藏层不保留选择覆盖。
        EditorTool::Domain(_) => domain.selected_layer.as_ref().filter(|id| domain.selection_info(DomainItemKind::Layer, id).is_some_and(|info| info.visible))
            .map(|id| (LayerTarget::Domain(id.clone()), None)),
        EditorTool::Water | EditorTool::RiverSpline | EditorTool::Waterfall => draft
            .water
            .as_ref()
            .and_then(|water| match editor.tool {
                EditorTool::Water => water
                    .layers
                    .get(editor.selected_water_layer)
                    .map(|layer| (WaterAuthoringFeatureKind::StillWater, &layer.id)), // 静水作者索引。
                EditorTool::RiverSpline => water
                    .rivers
                    .get(editor.selected_water_river)
                    .map(|river| (WaterAuthoringFeatureKind::River, &river.id)), // 河流作者索引。
                EditorTool::Waterfall => water
                    .waterfalls
                    .get(editor.selected_waterfall)
                    .map(|fall| (WaterAuthoringFeatureKind::Waterfall, &fall.id)), // 瀑布作者索引。
                _ => None,
            })
            .map(|(kind, id)| (LayerTarget::Water(kind, id.clone()), None)), // 侧栏与地图候选共用稳定身份。
        _ => return, // 雕刻等工具不制造作者层。
    };
    state.clear(); // 显式侧栏操作淘汰旧候选及迟到视口点击。
    state.identity = Some(super::layers::Identity {
        map: draft.map_id.clone(),
        revision: session.revision,
        placement_derived_pose_revision: session.placement_derived_pose_revision,
        domain_hash: draft.adopted_domain_hash().to_owned(),
        generation: domain.observed_generation,
        ticket: state.ticket,
        filter: modes.selection_filter(&editor),
    }); // 侧栏选择同样不能跨草稿代次延续。
    if let Some((target, index)) = selected {
        state.selected = Some(target);
        state.selected_author_index = index;
        state.canvas_terrain = draft.terrain_documents.active.as_ref().map(|active| active.definition.id.clone());
    } // 只发布选择，不改变图层或撤销历史。
}
use crate::authoring::BoundaryEditState; // 整路径选择不能武装控制点拖动。
use crate::authoring::EditorAuthoringSession;
use crate::game_authoring::map_interaction::{MapInteractionState, DomainItemKind}; // 外部作者层选择端口。
use crate::resources::{EditorViewportSelectionState, MapEditorState}; // 统一工具和旧植被输出覆盖。
use crate::types::EditorTool; // 工具记忆不等于立即绘制。
use crate::types::VegetationPreviewMode; // 选择独立作者画布而非合并输出。
use crate::types::WaterAuthoringFeatureKind; // 稳定水体类别决定下次编辑工具。
use crate::ui::water_feature_list::activate_water_feature; // 复用水体列表的索引与分页同步。
use crate::ui::{
    actions::activate_vegetation_layer,
    gltf_browser::EditorGltfBrowserState,
    sidebars::{EditorLeftSidebarTab, MapEditorSidebarState},
    vegetation_layers::VEGETATION_LAYER_ROW_COUNT,
}; // 复用现有侧栏与资产选择。
use crate::viewport::modes::{ViewportMode, ViewportModeState}; // 选择模式记住下一次编辑工具。
use crate::viewport::{SelectedPlacementViewportState, SelectedWaterViewportState}; // 清除互斥对象覆盖。
use bevy::prelude::*; // 只写编辑会话资源。
use feathrise_scene::DynamicMeshProfileRequests; // 树木层仍走现有正式原型加载。 // 已校验的后台候选。

#[allow(clippy::too_many_arguments)] // 将单选相关状态同步在同一个系统中。
pub(crate) fn apply_layer_candidate(
    mut state: ResMut<LayerCandidateState>,
    mut editor: ResMut<MapEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut domain: ResMut<MapInteractionState>,
    mut sidebars: ResMut<MapEditorSidebarState>,
    mut legacy: ResMut<EditorViewportSelectionState>,
    mut browser: ResMut<EditorGltfBrowserState>,
    mut placement: ResMut<SelectedPlacementViewportState>,
    mut water: ResMut<SelectedWaterViewportState>,
    mut boundary: ResMut<BoundaryEditState>, // 玩法路径索引与作者侧栏共享。
    mut portal: ResMut<crate::viewport::SelectedPortalViewportState>, // 清除旧列表双击而不触发传送。
    mut profiles: Option<ResMut<DynamicMeshProfileRequests>>,
    mut modes: ResMut<ViewportModeState>,
    mut multi_selection: Option<ResMut<crate::viewport::multi_select::PlacementSelectionSet>>,
    mut sub_maps: Option<ResMut<crate::ui::sub_maps::SubMapEditorState>>,
    water_source: (
        Option<ResMut<crate::ui::terrain_objects::TerrainObjectsEditorState>>,
        Option<ResMut<crate::authoring::terrain_documents::TerrainDocumentTask>>,
        crate::ui::water_feature_list::WaterSourceSelectionGuard,
    ), // 来源切换作为一个SystemParam组合，保持Bevy的16项系统参数预算。
) {
    let (mut terrains, mut documents, mut source_guard) = water_source;
    // 与贴地推进同属Tasks但不是同一串行链；poll之后、apply之前的派生换代也须撤销旧空命中。
    state.invalidate_derived_pose(&session, domain.observed_generation, modes.selection_filter(&editor));
    if editor.suppress_world_pick_this_frame {
        state.apply = None; // 取消/保存同帧不得采用迟到世界点选并跳到玩法对象。
        state.apply_candidate_index = None;
    }
    if modes.mode.transforms_placement() {
        state.apply = None;
        state.apply_candidate_index = None;
        state.hover = None;
        state.hover_author_index = None;
        state.clear_objects = false;
    } // 进入变换同帧也要拒绝旧选层/空命中，正式目标与场景树网格保持不变。
    // 所有变更仅影响编辑器资源。
    domain.suppress_layer_highlight = modes.mode == ViewportMode::Select
        && !matches!(
            state.hover.as_ref().or(state.selected.as_ref()),
            Some(LayerTarget::Domain(_))
        ); // 跨类别预高亮时只让当前类型亮起，退出恢复正常角色编辑。
    let hover = match state.hover.as_ref() {
        Some(LayerTarget::Domain(id)) => Some(id.clone()),
        _ => None,
    }; // 悬停其他类型时角色恢复原先正式层。
    if domain.hover_layer != hover {
        domain.hover_layer = hover;
    } // 视觉身份变化才触发角色覆盖重建。
    if state.clear_objects {
        if let Some(terrains) = terrains.as_mut() { terrains.selected = None; terrains.selected_index = None; }
        if let Some(sub_maps) = sub_maps.as_mut() { sub_maps.selected = None; }
        if state.additive && multi_selection.as_ref().is_some_and(|selection| !selection.members.is_empty()) {
            state.clear_objects = false;
            if let Some(selection) = multi_selection.as_ref() {
                crate::viewport::multi_select::publish_primary(selection, &mut browser, &mut state);
            }
            return; // Shift点击空白保持原选集，不能被异步空命中清除。
        }
        if let Some(selection) = multi_selection.as_mut() { selection.clear(); }
        state.selected = None;
        state.selected_placement_hit = None;
        state.confirmed_viewport_selection = false;
        editor.selected_current_portal_id = None; // 空命中同步取消玩法选择。
        boundary.clear_drag();
        portal.clear_click_sequence();
        legacy.clear_vegetation_selection();
        browser.clear_viewport_selection();
        placement.clear_click_sequence();
        water.clear_viewport_selection();
        editor.status = "此处没有符合筛选的作者层".to_owned();
        state.clear_objects = false;
    } // 完整空命中是明确取消，不保留旧对象高亮。
    let Some(mut target) = state.apply.take() else {
        state.apply_candidate_index = None;
        return;
    }; // 稳定帧不扫描数据或修改选择。
    let candidate_index = state.apply_candidate_index.take();
    if candidate_index.is_some() && state.identity.as_ref().is_none_or(|identity|
        identity.placement_derived_pose_revision != session.placement_derived_pose_revision) {
        editor.status = "候选姿态已变化，请重新点击视口".into();
        return; // 所有候选行都依赖点击姿态，普通资产也不能绕过画布类型门确认旧命中。
    }
    let author_index = if let Some(index) = candidate_index {
        let Some(candidate) = state.candidates.get(index).filter(|candidate| candidate.target == target) else { return; };
        candidate.author_index
    } else { state.selected_author_index };
    if state.additive && !matches!(target, LayerTarget::Placement(_))
        && let Some(selection) = multi_selection.as_ref().filter(|selection| !selection.members.is_empty()) {
        crate::viewport::multi_select::publish_primary(selection, &mut browser, &mut state);
        editor.status = "Shift增减选择只适用于普通摆放资产".into();
        return; // 来源恢复之前即拒绝跨类别增减，不能让保留的正式资产选集被动换槽。
    }
    if state.candidate_uses_canvas(&target) && candidate_index.is_some() {
        let valid_identity = session.draft.as_ref().is_some_and(|draft| state.identity.as_ref().is_some_and(|identity|
            editor.selected_map_id.as_ref() == Some(&draft.map_id)
                && identity.map == draft.map_id && identity.revision == session.revision
                && identity.domain_hash == draft.adopted_domain_hash() && identity.generation == domain.observed_generation
                && identity.filter == modes.selection_filter(&editor)));
        if !valid_identity {
            editor.status = "候选来源已变化，请重新点击视口".into();
            return;
        }
        if matches!(&target, LayerTarget::TerrainObject(id) if state.candidate_canvas.as_ref()
            .is_some_and(|source| source.id.as_str() == id && editor.terrain_hidden.contains(&source.id))) {
            editor.status = "目标已隐藏；请在场景树管理保护状态".into();
            return;
        }
        if let Err(error) = state.validate_candidate_source(session.draft.as_ref().unwrap(), &target, author_index) {
            editor.status = error;
            return;
        }
        let terrain = state.candidate_terrain(&target).cloned();
        let before_revision = session.revision;
        let busy = source_guard.busy(&session);
        if let Err(error) = crate::ui::water_feature_list::focus_author_source(&mut editor, &mut session,
            terrain.as_ref(), busy, terrains.as_deref_mut(), documents.as_deref_mut()) {
            editor.status = error;
            return;
        }
        if session.revision != before_revision {
            source_guard.retarget_terrain_canvas();
            state.continue_candidate_source(&session);
        }
    } // 同批候选只在完整资格核验后恢复其原槽；失败不能先改正式行、作者下标或画布。
    if let LayerTarget::RootWater(kind, id) = &target {
        target = LayerTarget::Water(*kind, id.clone());
    }
    let Some(draft) = session.draft.as_ref() else {
        return;
    }; // 切图边界失败关闭。
    if matches!(&target, LayerTarget::Surface(_) | LayerTarget::Vegetation(_) | LayerTarget::Water(..))
        && candidate_index.is_none() && !state.author_canvas_matches(draft) {
        return; // 相同作者ID在不同网格也不能串选，旧卡片不能借新活动画布取得属性权限。
    }
    let previous_tool = editor.tool; // 全局选择模式必须保持Inspect输入门。
    if let LayerTarget::Placement(id) = &target {
        let allowed = browser.placement_record_index(id, session.placement_structure_revision)
            .and_then(|index| draft.placements.placements.get(index))
            .is_some_and(|record| record.id == *id && record.editor.selectable());
        if !allowed { editor.status = "目标已隐藏、锁定或失效；请在场景树管理保护状态".into(); return; }
    }
    if let Some(selection) = multi_selection.as_mut() {
        if let LayerTarget::Placement(id) = &target {
            let record = browser.placement_record_index(id, session.placement_structure_revision)
                .and_then(|index| draft.placements.placements.get(index).map(|record| (index, record)))
                .filter(|(_, record)| record.id == *id);
            if let Some((index, record)) = record {
                if record.building_preset_id.is_some() && state.additive && !selection.members.is_empty() {
                    crate::viewport::multi_select::publish_primary(selection, &mut browser, &mut state);
                    editor.status = "多选仅支持普通资产；建筑继续使用单栋或院落校验".into(); return;
                }
                if record.building_preset_id.is_none() {
                    let position = match crate::authoring::placement_pose::placement_record_world_position(draft, record) {
                        Ok(position) => position,
                        Err(error) => {
                            selection.clear();
                            crate::viewport::multi_select::publish_primary(selection, &mut browser, &mut state);
                            placement.clear_click_sequence();
                            editor.status = error;
                            return; // 独立三维源失效时不把源局部原点发布成世界选择。
                        }
                    };
                    let member = crate::viewport::multi_select::SelectedMember { id: id.clone(), index,
                        position };
                    let removing = state.additive && selection.contains(id);
                    if let Err(error) = selection.click(member, state.additive, &session) { editor.status = error; return; }
                    if selection.members.len() != 1 || removing {
                        state.selected_placement_hit = None;
                        state.confirmed_viewport_selection = false;
                        crate::viewport::multi_select::publish_primary(selection, &mut browser, &mut state);
                        placement.clear_click_sequence();
                        sidebars.reveal_scene_properties(); // 增减选集也发布对象属性意图，残留资产选择不能夺回右栏。
                        editor.status = format!("已选择{}个普通资产；移动模式可整组拖动", selection.members.len()); return;
                    }
                } else { selection.clear(); }
            }
        } else if state.additive && !selection.members.is_empty() {
            crate::viewport::multi_select::publish_primary(selection, &mut browser, &mut state);
            editor.status = "Shift增减选择只适用于普通摆放资产".into(); return;
        } else { selection.clear(); }
    }
    if !matches!(&target, LayerTarget::SubMap(_)) { if let Some(sub_maps) = sub_maps.as_mut() { sub_maps.selected = None; } }
    let mut confirmed_placement_hit = None;
    let selected = match &target {
        LayerTarget::TerrainObject(id) => {
            let valid = author_index.and_then(|index| draft.manifest.terrains.as_ref()?.get(index))
                .is_some_and(|object| object.id.as_str() == id && !editor.terrain_hidden.contains(&object.id));
            if valid {
                if let Some(terrains) = terrains.as_mut() { terrains.selected = feathrise_map_content::TerrainId::parse(id).ok();
                    terrains.selected_index = author_index; terrains.reset_lod_selection(); }
                sidebars.reveal_scene_properties(); editor.status = "已选择网格，可调整位置、尺寸和参数".into();
            }
            valid
        }
        LayerTarget::SubMap(id) => {
            if let Some(record) = draft.manifest.sub_maps.iter().find(|record| record.id.as_str() == id) {
                if let Some(sub_maps) = sub_maps.as_mut() { sub_maps.map_id = Some(draft.map_id.clone()); sub_maps.selected = Some(record.id.clone()); }
                sidebars.reveal_scene_properties(); editor.status = format!("已选择子地图“{}”；整体移动或旋转，内部内容打开源图编辑", record.label); true
            } else { false }
        }
        LayerTarget::Portal(id) => {
            if let Some(record) = author_index
                .and_then(|index| draft.portals.portals.get(index))
                .filter(|record| record.id == *id)
            {
                editor.selected_current_portal_id = Some(id.clone()); // 同步当前传送点列表的稳定ID。
                portal.clear_click_sequence(); // 候选轮选不形成双击定位。
                editor.status = format!("已选择传送点“{}”；切到编辑模式可修改", record.label);
                true
            } else {
                false
            }
        }
        LayerTarget::Boundary(id) => {
            if let Some((index, path)) = author_index
                .and_then(|index| {
                    draft
                        .boundaries
                        .as_ref()?
                        .paths
                        .get(index)
                        .map(|path| (index, path))
                })
                .filter(|(_, path)| path.id == *id)
            {
                boundary.clear_drag(); // 只选择整条路径，不保留旧拖动快照。
                boundary.map_id = Some(draft.map_id.clone());
                boundary.path_index = Some(index); // point_index保持None，不能生成笔划。
                editor.status = format!("已选择边界“{}”；切到编辑模式可修改", path.label);
                true
            } else {
                false
            }
        }
        LayerTarget::Placement(id) => {
            let hit = if candidate_index.is_none() && state.selected.as_ref() == Some(&target) {
                state.selected_placement_hit.as_ref().filter(|hit| match hit {
                    super::placement::PlacementViewportHit::SceneMesh(selection) => selection.placement_id == *id,
                    super::placement::PlacementViewportHit::Primitive { placement_id, .. } => placement_id == id,
                }).cloned().or_else(|| state.placement_hits.get(id).cloned())
            } else { state.placement_hits.get(id).cloned() };
            if let Some(hit) = hit {
                confirmed_placement_hit = Some(hit.clone());
                placement.clear_click_sequence(); // 统一候选轮选不是旧对象双击。
                super::placement::select_placement_hit(
                    hit,
                    &draft.map_id,
                    0.0,
                    &mut editor,
                    &mut browser,
                    &mut placement,
                    &mut legacy,
                    &mut sidebars,
                ); // 场景树和属性区复用既有稳定选择入口。
                placement.clear_click_sequence(); // 列表确认只选择，不移动相机。
                editor.status = format!("已选择资产“{id}”；可在场景面板变换、复制或删除"); // 不误导用户再次点击定位。
                true
            } else {
                false // 过期精确命中不能从另一次点击猜测。
            }
        }
        LayerTarget::Surface(id) => {
            // 地表按真实作者ID和后台索引双校验。
            if let Some((index, layer)) = author_index
                .and_then(|index| {
                    draft
                        .surface
                        .as_ref()?
                        .layers
                        .get(index)
                        .map(|layer| (index, layer))
                })
                .filter(|(_, layer)| layer.id == *id)
            {
                // 不靠预设ID猜测。
                editor.selected_surface_layer = index; // 列表页和笔刷共享索引。
                editor.surface_selected_preset_id = Some(layer.preset_id.clone()); // 下次切编辑模式继续该作者层。
                editor.status = format!("已选择地表作者层“{}”；切到编辑模式可绘制", layer.label); // 清晰操作提示。
                true // 成功取得单选所有权。
            } else {
                false
            } // 过期下标不污染选择。
        }
        LayerTarget::Vegetation(id) => {
            // 同预设多个作者画布必须分别选中。
            if let Some((index, layer)) = author_index
                .and_then(|index| {
                    draft
                        .vegetation
                        .as_ref()?
                        .authoring
                        .as_ref()?
                        .layers
                        .get(index)
                        .map(|layer| (index, layer))
                })
                .filter(|(_, layer)| layer.id == *id && layer.enabled)
            {
                // 隐藏层不可进入。
                activate_vegetation_layer(&mut editor, &session, index, profiles.as_deref_mut()); // 保留正式Mesh原型预加载。
                editor.vegetation_layer_page = index / VEGETATION_LAYER_ROW_COUNT; // 展示真实作者行。
                editor.vegetation_preview_mode = VegetationPreviewMode::Source; // 不显示同预设的其他来源。
                editor.status = format!(
                    "已选择植被作者层“{}”{}",
                    layer.label,
                    if layer.locked {
                        "；该层已锁定，仅可查看"
                    } else {
                        "；切到编辑模式可绘制"
                    }
                ); // 锁定可查看但不解锁。
                true // 正式选择完成。
            } else {
                false
            } // 异步删除/隐藏后的候选失败关闭。
        }
        LayerTarget::Water(kind, id) => {
            if let Some(asset) = draft.water.as_ref()
                && activate_water_feature(&mut editor, asset, *kind, id).is_ok()
            {
                water.clear_viewport_selection(); // 候选轮选不触发旧水体双击定位。
                water.select_from_viewport(&draft.map_id, *kind, id, 0.0); // 正式身份与水体侧栏保持一致。
                water.clear_click_sequence(); // 列表确认只选中，不产生定位请求。
                editor.status = "已选择水体；切到编辑模式可修改".to_owned(); // 与其他候选统一提示。
                true
            } else {
                false // 删除或过期候选不修改正式选择。
            }
        }
        LayerTarget::RootWater(..) => false, // 上方已规范到恢复后的正式Water选择。
        LayerTarget::Domain(id) => {
            // 角色自身负责隐藏、锁定与索引校验。
            if session.draft.as_ref().is_some_and(|draft| domain.snapshot_for(draft).is_some()) && domain.select_layer(id) {
                editor.status = domain.status.clone();
                true
            } else {
                false
            } // 不生成/删除角色。
        }
    };
    editor.tool = previous_tool; // activate_vegetation_layer不能暗中切入绘制。
    if selected {
        // 点选只切到独立属性页；顶部创建模块与动态工具页签保留作者的显式选择。
        // 地表、传送和边界的唯一原表单由检查器移挂，不借工具页冒充属性。
        sidebars.reveal_scene_properties();
        // 同次发布单选身份与互斥清理。
        modes.remember_tool(match &target {
            LayerTarget::TerrainObject(_) => EditorTool::Raise,
            LayerTarget::Portal(_) => EditorTool::Portal,
            LayerTarget::Boundary(_) => EditorTool::Boundary,
            LayerTarget::Placement(_) | LayerTarget::SubMap(_) => EditorTool::Inspect, // 选择实例不武装放置新资产工具。
            LayerTarget::Surface(_) => EditorTool::Surface,
            LayerTarget::Vegetation(_) => EditorTool::Vegetation,
            LayerTarget::Domain(_) => EditorTool::Domain(domain.domain()),
            LayerTarget::Water(kind, _) | LayerTarget::RootWater(kind, _) => match kind {
                WaterAuthoringFeatureKind::StillWater => EditorTool::Water,
                WaterAuthoringFeatureKind::River => EditorTool::RiverSpline,
                WaterAuthoringFeatureKind::Waterfall => EditorTool::Waterfall,
            }, // 切编辑恢复水体所属工具。
        }); // 切编辑时准确回到该作者层所属工具。
        let selected_water = matches!(&target, LayerTarget::Water(..)); // 水体自身选择不能被通用清理抹掉。
        let selected_placement = matches!(&target, LayerTarget::Placement(_)); // 资产自己接管时保留刚发布的场景树选择。
        if !matches!(&target, LayerTarget::Portal(_)) {
            editor.selected_current_portal_id = None; // 跨类别不能遗留传送点标记。
            portal.clear_click_sequence();
        }
        if !matches!(&target, LayerTarget::Boundary(_)) {
            boundary.clear_drag();
        } // 旧边界拖动身份同步释放。
        let canvas_terrain = draft.terrain_documents.active.as_ref().map(|active| active.definition.id.clone());
        if let LayerTarget::Boundary(id) = &target {
            session.selected_boundary_path = Some((draft.map_id.clone(), id.clone())); // 记忆稳定路径，Inspect/松手清理拖动不丢正式选择。
        }
        state.selected_placement_hit = confirmed_placement_hit;
        state.confirmed_viewport_selection = candidate_index.is_some()
            || (state.confirmed_viewport_selection && state.selected.as_ref() == Some(&target));
        state.selected = Some(target); // 临时hover不覆盖这个正式身份。
        state.selected_author_index = author_index;
        if let Some(index) = candidate_index { state.index = index; state.page = index / 8; }
        state.canvas_terrain = canvas_terrain;
        state.selection_revision = state.selection_revision.wrapping_add(1); // 再次确认同一ID也可回到其属性页；侧栏切工具不能被旧选择每帧抢回。
        domain.suppress_layer_highlight = modes.mode == ViewportMode::Select
            && !matches!(
                state.hover.as_ref().or(state.selected.as_ref()),
                Some(LayerTarget::Domain(_))
            ); // 正式选层当帧同步高亮所有权。
        legacy.clear_vegetation_selection(); // 原正式输出层覆盖让给独立作者层。
        if !selected_placement {
            browser.clear_viewport_selection(); // 其他作者层才清除资产选择。
        }
        placement.clear_click_sequence(); // 旧对象双击不能穿越作者层选择。
        if !selected_water {
            water.clear_viewport_selection(); // 非水体作者层接管时才清理水体选择。
        }
    }
}
