//! F12独立水位图层的新建、复制和删除操作。

use feathrise_editor_ui::EditorButtonClick;
use std::collections::BTreeMap; // 水层ID到R8 Arc遮罩的确定性快照。
use std::sync::Arc; // 复制水层共享只读像素，首次绘制时写时复制。
use std::time::{SystemTime, UNIX_EPOCH}; // 新私有遮罩路径避开保留在磁盘的删除层。

use bevy::prelude::*; // Interaction与可变作者资源。
use bevy::ui::RelativeCursorPosition; // 状态提示区分光标是否正位于可绘制视口。
use feathrise_map_content::{
    MapId, MapWaterAsset, MapWaterEndpoint,
}; // 公共静水预算、完整校验与连接清理。

use crate::authoring::{
    EditorAuthoringSession, EditorCommand, WaterBrushStroke, WaterLayersChange,
    WaterNetworkEditState, default_water_layer, sync_water_network_connections, validate_water_network_edit,
}; // 水层结构与遮罩表作为一次可撤销事务。
use crate::components::{MapEditorActionButton, MapEditorViewport}; // 静态按钮和视口命中水位来源。
use crate::resources::{MapEditorNumericScrubMouseGuard, MapEditorState}; // 当前鼠标所有权、选择、状态和统一脏标记。
use crate::types::{EditorAction, EditorTool}; // 消费两个静水专用动作和列表级删除所选。
use crate::ui::water_actions::reject_water_feature_switch; // 增删静水与列表直选共享跨帧身份保护。

mod create; // 空白R8在单个后台任务中分配，完成后复验身份再形成一次撤销事务。

/// 菜单的静默请求已确认具体水域；静水复用完整遮罩事务，水网删除沿原控制点写侧。
pub(crate) fn handle_scene_menu(
    menu: Res<crate::ui::scene_context_menu::SceneContextMenuState>,
    mut editor: ResMut<MapEditorState>, mut session: ResMut<EditorAuthoringSession>,
    stroke: Res<WaterBrushStroke>, network: Res<WaterNetworkEditState>,
    mut terrains: Option<ResMut<crate::ui::terrain_objects::TerrainObjectsEditorState>>,
    mut documents: Option<ResMut<crate::authoring::terrain_documents::TerrainDocumentTask>>,
    source_guard: crate::ui::water_feature_list::WaterSourceSelectionGuard,
) {
    use crate::ui::scene_context_menu::{SceneMenuAction, SceneObjectKey};
    use crate::types::{WaterAuthoringFeatureKind, WaterNetworkAction};
    let Some(request) = menu.ready.as_ref().filter(|request| request.target.current(&editor, &session) && request.target.allows(request.action)) else { return; };
    let SceneObjectKey::Water { kind, index, id, terrain } = &request.target.key else { return; };
    if !matches!(request.action, SceneMenuAction::Copy | SceneMenuAction::Delete)
        || reject_water_feature_switch(&mut editor, &session, &stroke, &network) { return; }
    let old_tool = editor.tool;
    let busy = source_guard.busy(&session);
    if let Err(error) = crate::ui::water_feature_list::activate_listed_water_feature(&mut editor, &mut session,
        terrain.as_ref(), busy, *kind, id, terrains.as_deref_mut(), documents.as_deref_mut()) {
        editor.status = error;
        return;
    }
    editor.tool = old_tool; // 菜单写侧也复验并恢复来源，独立调用不能误改同ID私有层。
    if *kind == WaterAuthoringFeatureKind::StillWater {
        let action = if request.action == SceneMenuAction::Copy { EditorAction::WaterDuplicateLayer } else { EditorAction::WaterDeleteSelected };
        let old_tool = editor.tool;
        apply_and_present_water_structure_action(action, *index, None, None, false, &mut editor, &mut session, None);
        editor.tool = old_tool; // 对象管理不启用静水笔刷。
    } else if request.action == SceneMenuAction::Delete {
        editor.water_network_action_requested = Some(match kind {
            WaterAuthoringFeatureKind::River => WaterNetworkAction::RiverDelete,
            WaterAuthoringFeatureKind::Waterfall => WaterNetworkAction::WaterfallDelete,
            WaterAuthoringFeatureKind::StillWater => unreachable!(),
        });
    }
}

/// 空白R8从按钮到完成值始终冻结地图、可选私有来源、完整姿态和作者修订。
#[derive(Clone, Debug, PartialEq, Eq)]
struct PendingClipmapWaterLayerRequest {
    map_id: MapId,
    canvas: crate::authoring::water_canvas::WaterCanvasIdentity,
    source_layer_id: Option<String>,
    preset_id: Option<String>,
    authoring_revision: u64,
}

impl PendingClipmapWaterLayerRequest {
    fn matches_canvas(&self, draft: &crate::authoring::EditorMapDraft) -> bool {
        draft.map_id == self.map_id && self.canvas == crate::authoring::water_canvas::identity(draft)
    }
}

#[derive(Resource, Default)]
pub(crate) struct PendingClipmapWaterLayerCreate {
    blank: Option<create::BlankCreation>, // 单个分配/退役槽；新建不再等待地形画布或地面命中。
    legacy: Option<LegacyWaterCreation>, // 旧nonEmpty容器仍由原primary水文档拥有，只有它确实未载入时复用单槽读取。
}

struct LegacyWaterCreation {
    map: MapId,
    object: Arc<feathrise_map_content::MapTerrainObject>,
    index: usize,
    revision: u64,
    adopted: bool,
    preset: Option<String>,
}
impl PendingClipmapWaterLayerCreate {
    pub(crate) fn terrain_canvas_adopted(&mut self, map: &MapId, object: &feathrise_map_content::MapTerrainObject,
        index: usize, before: u64, after: u64) {
        if let Some(wait) = self.legacy.as_mut().filter(|wait| !wait.adopted && wait.map == *map
            && wait.object.as_ref() == object && wait.index == index && wait.revision == before) {
            wait.revision = after;
            wait.adopted = true;
        }
    }
    pub(crate) fn terrain_canvas_failed(&mut self, map: &MapId, object: &feathrise_map_content::MapTerrainObject, index: usize, revision: u64) {
        if self.legacy.as_ref().is_some_and(|wait| !wait.adopted && wait.map == *map
            && wait.object.as_ref() == object && wait.index == index && wait.revision == revision) {
            self.legacy = None;
        }
    }
}

fn observe_legacy_creation(pending: &mut PendingClipmapWaterLayerCreate, editor: &MapEditorState,
    session: &EditorAuthoringSession) {
    if pending.legacy.as_ref().is_some_and(|wait| !editor.open || editor.close_requested || editor.save_requested
        || session.saving || session.water_preset_delete_pending || editor.tool != EditorTool::Water || editor.map_transition_in_progress
        || editor.pending_map_transition.is_some() || editor.selected_map_id.as_ref() != Some(&wait.map)
        || session.revision != wait.revision || editor.terrain_canvas_target.as_ref() != Some(&wait.object.id)) {
        pending.legacy = None; // 输入暂停仍永久取消，迟到载入不能复活原按钮。
    }
}

/// 新建根水直接使用地图文档，旧私有画布停放后仍可保存、切回和撤销。
fn focus_root_water(editor: &mut MapEditorState, session: &mut EditorAuthoringSession,
    selection: Option<&mut crate::ui::terrain_objects::TerrainObjectsEditorState>,
    documents: Option<&mut crate::authoring::terrain_documents::TerrainDocumentTask>, source_busy: bool) -> bool {
    if session.saving || session.water_preset_delete_pending { return false; }
    let switching = session.draft.as_ref().is_some_and(|draft| {
        draft.terrain_documents.active.as_ref().is_some_and(|active| {
            draft.manifest.terrain.is_empty()
                || (draft.manifest.terrains.is_none() && active.definition.id != feathrise_map_content::TerrainId::legacy())
        })
    });
    if switching && (source_busy || selection.as_ref().is_some_and(|selection| selection.busy())) {
        editor.status = "当前画布仍有编辑事务，请完成后再新建独立静水".into();
        return false;
    }
    if switching && session.sparse_terrain_dirty {
        // 稀疏高度页不存放在TerrainCanvas中，未保存时切到根水会使原作者状态退役。
        editor.status = "当前高度作者源尚未保存；保存完成后可新建独立静水".into();
        return false;
    }
    crate::authoring::terrain_documents::scope_history(session);
    if switching && session.draft.as_ref().is_some_and(|draft| draft.terrain_documents.scoped_through < session.undo.len()) {
        // 每帧只整理64条；旧私有历史全部取得来源前不能停放，否则余下命令会误归根水。
        editor.status = "正在整理当前画布撤销记录，请稍后重新新建静水".into();
        return false;
    }
    let revision = session.water_revision;
    let Some(draft) = session.draft.as_mut() else { return false; };
    if !draft.manifest.is_data_driven() {
        editor.status = "当前地图没有可编辑水域文档".into();
        return false;
    }
    if !draft.manifest.terrain.is_empty() {
        if draft.manifest.terrains.is_none() {
            if let Err(error) = crate::authoring::terrain_documents::restore_legacy(draft) {
                editor.status = error;
                return false;
            }
            editor.terrain_canvas_target = None;
            if draft.water.is_none() {
                draft.water = Some(crate::authoring::default_water_asset(&draft.map_id));
                draft.water_catalog_registration_required = true;
            }
            return true; // 旧单地形本就使用主作者槽，不能临时认领后又被focus恢复而使R8请求跨源。
        }
        // 旧单地形/兼容容器继续使用已载入的原primary文档，绝不隐式迁移用户地图。
        if let Some(active) = draft.terrain_documents.active.as_ref() {
            editor.terrain_canvas_target = Some(active.definition.id.clone());
            if draft.water.is_none() {
                draft.water = Some(crate::authoring::default_water_asset(&draft.map_id));
                draft.water_catalog_registration_required = true;
            }
            return true;
        }
        editor.status = "旧地图主水画布尚未载入，请先选择主网格".into();
        return false;
    }
    crate::authoring::terrain_documents::root_water::remember_revision(draft, revision);
    let changed = crate::authoring::terrain_documents::root_water::focus(draft);
    if let Some(selection) = selection {
        selection.selected = None;
        selection.selected_index = None;
        selection.documents_loading = false;
    }
    if let Some(documents) = documents { documents.requested = None; }
    // 在途网格载入仍占据原单槽；清选择和换代使其迟到结果只能退役，不能抢回根水。
    draft.terrain_documents.loading = false;
    editor.terrain_canvas_target = None;
    if draft.water.is_none() {
        draft.water = Some(crate::authoring::default_water_asset(&draft.map_id));
        draft.water_asset_path = format!("{}/{}", draft.folder.trim_end_matches('/'), feathrise_map_content::MAP_WATER_FILE_NAME);
        draft.water_catalog_registration_required = true;
    }
    if changed {
        session.revision = session.revision.wrapping_add(1);
        session.surface_revision = session.surface_revision.wrapping_add(1);
        session.vegetation_revision = session.vegetation_revision.wrapping_add(1);
        session.vegetation_structure_revision = session.vegetation_structure_revision.wrapping_add(1);
        session.water_revision = session.water_revision.wrapping_add(1);
    }
    true
}

/// 数值输入占有鼠标或普通输入暂停时仍记录取消，不推进大像素任务。
pub(crate) fn observe_water_creation_cancellation(
    mut pending: ResMut<PendingClipmapWaterLayerCreate>,
    editor: Res<MapEditorState>, session: Res<EditorAuthoringSession>,
) {
    create::observe(&mut pending, &editor, &session);
    observe_legacy_creation(&mut pending, &editor, &session);
}

/// 新建直接创建地图根静水；复制/删除仍作用当前根水或旧网格私有水。
pub(crate) fn handle_water_layer_buttons(
    buttons: Query<(&EditorButtonClick, &MapEditorActionButton), Changed<EditorButtonClick>>,
    numeric_scrub_mouse: Res<MapEditorNumericScrubMouseGuard>,
    viewport: Query<&RelativeCursorPosition, With<MapEditorViewport>>,
    mut editor: ResMut<MapEditorState>, mut session: ResMut<EditorAuthoringSession>,
    stroke: Res<WaterBrushStroke>, network_edit: Res<WaterNetworkEditState>,
    mut pending_create: ResMut<PendingClipmapWaterLayerCreate>,
    mut mode: Option<ResMut<crate::viewport::modes::ViewportModeState>>,
    mut terrain_selection: Option<ResMut<crate::ui::terrain_objects::TerrainObjectsEditorState>>,
    mut terrain_documents: Option<ResMut<crate::authoring::terrain_documents::TerrainDocumentTask>>,
    source_guard: crate::ui::water_feature_list::WaterSourceSelectionGuard,
) {
    if numeric_scrub_mouse.blocked() { return; }
    create::poll(&mut pending_create, &mut editor, &mut session);
    observe_legacy_creation(&mut pending_create, &editor, &session);
    if pending_create.legacy.as_ref().is_some_and(|wait| wait.adopted) {
        let wait = pending_create.legacy.take().unwrap();
        let source_busy = source_guard.busy(&session);
        if !focus_root_water(&mut editor, &mut session, terrain_selection.as_deref_mut(), terrain_documents.as_deref_mut(), source_busy) { return; }
        let selected = session.draft.as_ref().and_then(|draft| draft.water.as_ref())
            .map_or(0, |water| editor.selected_water_layer.min(water.layers.len().saturating_sub(1)));
        editor.selected_water_layer = selected;
        create::begin(selected, wait.preset.as_deref(), None, &mut pending_create, &mut editor, &session, None);
    }
    for (interaction, button) in &buttons {
        if !interaction.is_clicked() || !water_structure_action(button.0, editor.tool)
            || reject_water_feature_switch(&mut editor, &session, &stroke, &network_edit) { continue; }
        if button.0 == EditorAction::WaterAddLayer {
            if pending_create.blank.is_some() {
                editor.status = "静水层遮罩正在后台准备，完成后可继续新建".into();
                continue;
            }
            let legacy = session.draft.as_ref().filter(|draft| !draft.manifest.terrain.is_empty())
                .and_then(|draft| draft.manifest.terrains.as_ref())
                .and_then(|objects| objects.iter().enumerate().find(|(_, object)| object.id == feathrise_map_content::TerrainId::legacy()))
                .map(|(index, object)| (index, Arc::new(object.clone())));
            if let Some((index, object)) = legacy
                && session.draft.as_ref().is_some_and(|draft| draft.terrain_documents.active.as_ref()
                    .is_none_or(|active| active.definition.id != object.id)) {
                if let (Some(selection), Some(documents), Some(draft)) = (terrain_selection.as_deref_mut(), terrain_documents.as_deref_mut(), session.draft.as_ref()) {
                    if crate::authoring::terrain_documents::request_focus(documents, selection, draft, &object.id) {
                        editor.terrain_canvas_target = Some(object.id.clone());
                        pending_create.legacy = Some(LegacyWaterCreation { map: draft.map_id.clone(), object, index,
                            revision: session.revision, adopted: false, preset: editor.water_selected_preset_id.clone() });
                        begin_water_creation_mode(&mut editor, mode.as_deref_mut());
                        editor.status = "正在载入旧地图原主水文档，就绪后继续新建静水".into();
                        continue;
                    }
                }
                continue;
            }
            pending_create.legacy = None;
            let source_busy = source_guard.busy(&session);
            if !focus_root_water(&mut editor, &mut session, terrain_selection.as_deref_mut(), terrain_documents.as_deref_mut(), source_busy) { continue; }
            begin_water_creation_mode(&mut editor, mode.as_deref_mut());
        }
        let selected = session.draft.as_ref().and_then(|draft| draft.water.as_ref())
            .map_or(0, |water| editor.selected_water_layer.min(water.layers.len().saturating_sub(1)));
        let preset = editor.water_selected_preset_id.clone();
        if button.0 == EditorAction::WaterAddLayer {
            editor.selected_water_layer = selected;
            // 侧栏按钮没有地面落点；默认水位来自层模板，不借旧鼠标位置或零平面命中。
            create::begin(selected, preset.as_deref(), None, &mut pending_create, &mut editor, &session, None);
        } else {
            apply_and_present_water_structure_action(button.0, selected, preset.as_deref(), None,
                viewport.single().is_ok_and(|cursor| cursor.cursor_over()), &mut editor, &mut session, None);
        }
    }
}

/// 只有明确的新建动作进入编辑；普通水域工具页导航仍由全局模式门维持选择模式。
fn begin_water_creation_mode(editor: &mut MapEditorState, mode: Option<&mut crate::viewport::modes::ViewportModeState>) {
    editor.tool = EditorTool::Water;
    if let Some(mode) = mode {
        mode.mode = crate::viewport::modes::ViewportMode::Edit;
        mode.remember_tool(EditorTool::Water);
    }
}

fn apply_and_present_water_structure_action(
    action: EditorAction,
    selected: usize,
    selected_preset_id: Option<&str>,
    cursor_level: Option<f32>,
    cursor_over_viewport: bool,
    editor: &mut MapEditorState,
    session: &mut EditorAuthoringSession,
    ready_mask: Option<Arc<Vec<u8>>>,
) {
    match apply_water_structure_action(action, selected, selected_preset_id, cursor_level, session, ready_mask)
    {
        Ok((selected, status)) => {
            editor.selected_water_layer = selected;
            editor.tool = EditorTool::Water;
            editor.water_selected_preset_id = session
                .draft
                .as_ref()
                .and_then(|draft| draft.water.as_ref())
                .and_then(|water| water.layers.get(selected))
                .and_then(|layer| (!layer.preset_id.is_empty()).then(|| layer.preset_id.clone())); // 新建、复制后左栏立即指向实际保存的预设身份。
            editor.status = if cursor_over_viewport {
                status
            } else {
                format!("{status}；把光标移入视口后可继续绘制")
            };
            session.mark_water_layers_changed(editor); // 只在完整草稿已经原子采用后换代预览。
        }
        Err(error) => editor.status = error,
    }
}

fn water_structure_action(action: EditorAction, tool: EditorTool) -> bool {
    matches!(
        action,
        EditorAction::WaterAddLayer | EditorAction::WaterDuplicateLayer
    ) || (action == EditorAction::WaterDeleteSelected && tool == EditorTool::Water) // 通用删除只有在静水行高亮时进入R8图层事务。
}

fn apply_water_structure_action(
    action: EditorAction,
    selected: usize,
    selected_preset_id: Option<&str>,
    cursor_level: Option<f32>,
    session: &mut EditorAuthoringSession,
    ready_mask: Option<Arc<Vec<u8>>>,
) -> Result<(usize, String), String> {
    if session.water_preset_delete_pending {
        return Err("水体预设删除事务尚未完成，暂不能修改水域图层".into());
    }
    if session.saving {
        return Err("保存期间不能修改水域图层".to_owned());
    }
    let (map_id, folder, before, before_masks, root_bounds) = {
        let draft = session
            .draft
            .as_ref()
            .ok_or_else(|| "地图作者数据尚未就绪".to_owned())?;
        (
            draft.map_id.clone(),
            draft.terrain_folder().to_owned(),
            draft
                .water
                .clone()
                .ok_or_else(|| "当前地图没有可编辑水域文件".to_owned())?,
            draft.water_masks.clone(), // 只复制Arc句柄，删除层仍可立即撤销。
            draft.terrain_documents.active.is_none().then(|| root_water_bounds(&draft.manifest)),
        )
    };
    let mut after = before.clone();
    let mut after_masks = before_masks.clone();
    let selected = selected.min(after.layers.len().saturating_sub(1));
    let (next_selected, status) = match action {
        EditorAction::WaterAddLayer => add_water_layer_with_mask(
            selected,
            cursor_level,
            selected_preset_id,
            &folder,
            &mut after,
            &mut after_masks,
            ready_mask.ok_or_else(|| "空白水域遮罩尚未完成后台准备".to_owned())?,
        )?,
        EditorAction::WaterDuplicateLayer => {
            duplicate_water_layer(selected, &folder, &mut after, &mut after_masks)?
        }
        EditorAction::WaterDeleteSelected => {
            delete_water_layer(selected, &mut after, &mut after_masks)?
        }
        _ => return Err("当前按钮不是水层结构操作".to_owned()),
    };
    if action == EditorAction::WaterAddLayer && let Some(bounds) = root_bounds {
        // 首次创建把R8地址域写进水层，不能以空容器的1米占位范围限制静水画布。
        after.layers[next_selected].canvas_bounds.get_or_insert(bounds);
    }
    sync_water_network_connections(&mut after); // 静水结构变化与河流/瀑布断连或重吸附进入同一完整快照。
    let mut still_water_only = after.clone();
    still_water_only.rivers.clear();
    still_water_only.waterfalls.clear();
    still_water_only.validate(&map_id)?; // 静水层自身仍走正式路径、分辨率、水位和材质校验。
    validate_water_network_edit(&after, &map_id)?; // 未完成河流或瀑布不阻止作者继续修整静水连接。
    validate_water_masks(&after, &after_masks)?;
    let draft = session
        .draft
        .as_mut()
        .ok_or_else(|| "地图作者数据在水层操作期间失效".to_owned())?;
    draft.water = Some(after.clone());
    draft.water_masks = after_masks.clone(); // 描述与像素表在同一系统中原子替换。
    session.push_bounded_water_history(EditorCommand::WaterLayersChanged(Box::new(
        WaterLayersChange {
            before,
            after,
            before_masks,
            after_masks,
        })),
    );
    Ok((next_selected, status))
}

fn root_water_bounds(manifest: &feathrise_map_content::MapManifestAsset) -> [[f32; 2]; 2] {
    let min = manifest.world_min;
    let max = manifest.world_max;
    if min.iter().chain(max.iter()).all(|value| value.is_finite())
        && min[0] < max[0] && min[1] < max[1]
        && (min != [-0.5; 2] || max != [0.5; 2]) {
        [min, max]
    } else { [[-128.0; 2], [128.0; 2]] } // 空地图默认以地图原点为中心的256米画布。
}

#[cfg(test)]
fn add_water_layer(selected: usize, cursor_level: Option<f32>, preset: Option<&str>, folder: &str,
    water: &mut MapWaterAsset, masks: &mut BTreeMap<String, Arc<Vec<u8>>>) -> Result<(usize, String), String> {
    let resolution = water.layers.get(selected).map_or_else(|| default_water_layer(folder).resolution, |s| s.resolution);
    add_water_layer_with_mask(selected, cursor_level, preset, folder, water, masks, create::test_blank(resolution))
}

fn add_water_layer_with_mask(
    selected: usize,
    cursor_level: Option<f32>,
    selected_preset_id: Option<&str>,
    folder: &str,
    water: &mut MapWaterAsset,
    masks: &mut BTreeMap<String, Arc<Vec<u8>>>,
    blank: Arc<Vec<u8>>,
) -> Result<(usize, String), String> {
    let mut layer = water
        .layers
        .get(selected)
        .cloned()
        .unwrap_or_else(|| default_water_layer(folder)); // 零层时只在作者点击后实例化首张默认规格画布。
    let id = unique_water_id(water);
    layer.id = id.clone();
    layer.label = format!("静水体 {}", water.layers.len() + 1);
    layer.mask_source_path = format!("{}/water/{id}.r8", folder.trim_end_matches('/'));
    layer.mask_texture_path = format!("{}/water/{id}.ktx2", folder.trim_end_matches('/'));
    if let Some(preset_id) = selected_preset_id {
        layer.preset_id = preset_id.to_owned(); // 作者先选预设再新建时，新静水直接引用该外观而不是复制旧层选择。
    }
    if let Some(level) = cursor_level {
        layer.water_level = level; // 新水层可直接取当前高处水潭或低处河道的光标高度。
    }
    let count = pixel_count(layer.resolution)?;
    if blank.len() != count {
        return Err("已完成空白水域遮罩与新层分辨率不匹配".to_owned());
    }
    let index = if water.layers.is_empty() {
        0
    } else {
        selected + 1
    };
    let label = layer.label.clone();
    water.layers.insert(index, layer);
    masks.insert(id, blank); // 后台完成值移入草稿，主帧不清零大像素缓冲。
    Ok((
        index,
        format!("已新建“{label}”；该静水体拥有独立水位和R8覆盖"),
    ))
}

fn duplicate_water_layer(
    selected: usize,
    folder: &str,
    water: &mut MapWaterAsset,
    masks: &mut BTreeMap<String, Arc<Vec<u8>>>,
) -> Result<(usize, String), String> {
    let source = water
        .layers
        .get(selected)
        .cloned()
        .ok_or_else(|| "当前没有可复制静水体".to_owned())?;
    let pixels = masks
        .get(&source.id)
        .cloned()
        .ok_or_else(|| format!("水域层{}缺少R8遮罩", source.id))?;
    let id = unique_water_id(water);
    let mut duplicate = source.clone();
    duplicate.id = id.clone();
    duplicate.label = format!("{} · 副本", source.label);
    duplicate.mask_source_path = format!("{}/water/{id}.r8", folder.trim_end_matches('/'));
    duplicate.mask_texture_path = format!("{}/water/{id}.ktx2", folder.trim_end_matches('/'));
    water.layers.insert(selected + 1, duplicate);
    masks.insert(id, pixels); // 两层首次修改前共享遮罩，水位参数已经彼此独立。
    Ok((
        selected + 1,
        format!("已复制静水体“{}”；可单独调整副本水位", source.label),
    ))
}

fn delete_water_layer(
    selected: usize,
    water: &mut MapWaterAsset,
    masks: &mut BTreeMap<String, Arc<Vec<u8>>>,
) -> Result<(usize, String), String> {
    if water.layers.is_empty() {
        return Err("当前没有可删除静水体".to_owned());
    }
    let removed = water.layers.remove(selected);
    masks.remove(&removed.id); // 不在UI帧删除磁盘源，撤销快照继续拥有Arc像素。
    for river in &mut water.rivers {
        if river.source_layer_id.as_deref() == Some(removed.id.as_str()) {
            river.source_layer_id = None; // 删除静水源时同次清理河流悬空连接。
        }
        if river.target_layer_id.as_deref() == Some(removed.id.as_str()) {
            river.target_layer_id = None; // 删除静水目标时同次清理河流悬空连接。
        }
    }
    for waterfall in &mut water.waterfalls {
        if matches!(&waterfall.source, Some(MapWaterEndpoint::StillWater { layer_id }) if layer_id == &removed.id)
        {
            waterfall.source = None; // 瀑布源连接随静水删除原子断开。
        }
        if matches!(&waterfall.target, Some(MapWaterEndpoint::StillWater { layer_id }) if layer_id == &removed.id)
        {
            waterfall.target = None; // 瀑布目标连接随静水删除原子断开。
        }
    }
    Ok((
        selected.min(water.layers.len().saturating_sub(1)), // 删除末层后选择归零，但零不代表存在对象。
        format!("已删除静水体“{}”并断开相关水网连接", removed.label),
    ))
}

fn validate_water_masks(
    water: &MapWaterAsset,
    masks: &BTreeMap<String, Arc<Vec<u8>>>,
) -> Result<(), String> {
    for layer in &water.layers {
        let expected = pixel_count(layer.resolution)?;
        let actual = masks
            .get(&layer.id)
            .ok_or_else(|| format!("水域层{}缺少R8遮罩", layer.id))?
            .len();
        if actual != expected {
            return Err(format!(
                "水域层{}为{actual}字节，期望{expected}字节",
                layer.id
            ));
        }
    }
    Ok(())
}

fn unique_water_id(water: &MapWaterAsset) -> String {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let mut suffix = 0_u32;
    loop {
        let id = if suffix == 0 {
            format!("water-{stamp:x}")
        } else {
            format!("water-{stamp:x}-{suffix}")
        };
        if !water.layers.iter().any(|layer| layer.id == id)
            && !water.rivers.iter().any(|river| river.id == id)
            && !water.waterfalls.iter().any(|waterfall| waterfall.id == id)
        {
            return id;
        }
        suffix = suffix.saturating_add(1);
    }
}

fn pixel_count(resolution: [u32; 2]) -> Result<usize, String> {
    (resolution[0] as usize)
        .checked_mul(resolution[1] as usize)
        .ok_or_else(|| "水域遮罩分辨率乘法溢出".to_owned())
}

#[cfg(test)]
#[path = "water_layers_tests.rs"]
pub(crate) mod tests;
