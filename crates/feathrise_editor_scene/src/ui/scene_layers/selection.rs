//! 新增作者行与右键菜单共用同一选择桥，保留选择模式并恢复对应的下一次编辑工具。

use crate::{
    authoring::{EditorAuthoringSession, terrain_documents::{TerrainDocumentTask, TreeVegetationAction, TreeVegetationRequest}},
    game_authoring::scene_interaction::SceneInteractionState,
    resources::{EditorViewportSelectionState, SceneEditorState},
    types::{EditorTool, VegetationPreviewMode},
    ui::scene_context_menu::{SceneObjectKey, SceneObjectTarget},
    viewport::{
        LayerCandidateState, LayerTarget, SelectedWaterViewportState, modes::ViewportModeState,
    },
};
use bevy::{ecs::system::SystemParam, prelude::*};
use super::catalog::VegetationCatalogAssets;

#[derive(SystemParam)]
pub(crate) struct LayerSelectionContext<'w> {
    pub domain: Option<ResMut<'w, SceneInteractionState>>,
    pub modes: Option<ResMut<'w, ViewportModeState>>,
    water: Option<ResMut<'w, SelectedWaterViewportState>>,
    profiles: Option<ResMut<'w, feathrise_scene::DynamicMeshProfileRequests>>,
    sidebar: Option<ResMut<'w, crate::ui::sidebars::SceneEditorSidebarState>>,
    documents: Option<ResMut<'w, TerrainDocumentTask>>,
    catalog: VegetationCatalogAssets<'w>,
    source_guard: crate::ui::water_feature_list::WaterSourceSelectionGuard<'w>,
}

impl LayerSelectionContext<'_> {
    pub(crate) fn request_vegetation(
        &mut self, target: SceneObjectTarget, editor: &mut SceneEditorState, session: &EditorAuthoringSession,
        candidates: Option<&LayerCandidateState>, action: TreeVegetationAction,
    ) {
        let Some(documents) = self.documents.as_deref_mut() else { return; };
        request_cold_vegetation(target, editor, session, &self.catalog, documents, candidates, action);
    }

    pub(crate) fn take_vegetation_selection(&mut self, candidates: Option<&LayerCandidateState>) -> Option<TreeVegetationRequest> {
        self.documents.as_deref_mut()?.vegetation_ready.take_if(|intent| intent.action.selects())
            .filter(|intent| intent.selection_ticket == candidates.map(LayerCandidateState::selection_ticket))
    }

    pub(crate) fn cancel_vegetation(&mut self, session: &mut EditorAuthoringSession) {
        if let Some(documents) = self.documents.as_deref_mut() {
            documents.cancel_vegetation(session.draft.as_mut());
        }
    }

    /// 跨类别选择同时淘汰旧角色拾取和水域定位，避免迟到结果抢回属性或镜头。
    pub(crate) fn finish_selection(&mut self, key: &SceneObjectKey) {
        if let Some(sidebar) = self.sidebar.as_mut() {
            sidebar.reveal_scene_properties();
        } // 左树选择/右键命令只揭示属性，不重置顶部模块或左侧导航。
        let external = matches!(
            key,
            SceneObjectKey::Domain { .. }
        );
        if let Some(state) = self.domain.as_mut() {
            state.suppress_layer_highlight = !external;
            state.hover_layer = None;
            if !external { state.cancel_pick(); }
        }
        if !matches!(key, SceneObjectKey::Water { .. })
            && let Some(state) = self.water.as_mut()
        {
            state.clear_viewport_selection();
        }
    }
}

pub(in crate::ui) fn request_cold_vegetation(
    target: SceneObjectTarget, editor: &mut SceneEditorState, session: &EditorAuthoringSession,
    catalog: &VegetationCatalogAssets, documents: &mut TerrainDocumentTask,
    candidates: Option<&LayerCandidateState>, mut action: TreeVegetationAction,
) {
    let SceneObjectKey::Vegetation { index, id, output, cold: Some(_), .. } = &target.key else { return; };
    let current = session.draft.as_ref().and_then(|draft| catalog.row_definition(draft, &target.key));
    let valid = current.is_some_and(|asset| if *output {
        action.selects() && asset.authoring.is_none()
            && asset.layers.get(*index).is_some_and(|layer| &layer.id == id)
    } else {
        asset.authoring.as_ref().and_then(|authoring| authoring.layers.get(*index))
            .is_some_and(|layer| &layer.id == id && (!layer.locked
                || !matches!(action, TreeVegetationAction::Author(edit) if edit != crate::types::EditorAction::VegetationToggleLocked)))
    });
    if !target.current(editor, session) || !valid {
        editor.status = "植被图层来源已变化，请重新选择当前列表项".into();
        return;
    }
    if session.sparse_terrain_dirty {
        editor.status = "当前高度作者源尚未保存，请保存后再选择该植被图层".into();
        return;
    }
    if action == TreeVegetationAction::Select && documents.vegetation.as_ref().is_some_and(|pending|
        pending.target == target && pending.action == TreeVegetationAction::Focus)
    {
        action = TreeVegetationAction::Focus;
    } // 同源连点复用唯一完整画布加载，允许把尚未执行的选择升级为聚焦。
    documents.requested = None;
    documents.vegetation_ready = None;
    documents.vegetation = Some(TreeVegetationRequest {
        target, action, selection_ticket: candidates.map(LayerCandidateState::selection_ticket),
    });
    editor.status = "正在载入所选植被的作者画布".into();
}

pub(crate) fn select(
    target: &SceneObjectTarget,
    editor: &mut SceneEditorState,
    session: &mut EditorAuthoringSession,
    terrains: Option<&mut crate::ui::terrain_objects::TerrainObjectsEditorState>,
    candidates: Option<&mut LayerCandidateState>,
    mut legacy: Option<&mut EditorViewportSelectionState>,
    context: &mut LayerSelectionContext,
) -> bool {
    // 稳定行引用可能属于已停放的画布。完整恢复成功后，原属性编辑器才能使用当前作者索引。
    let previous_tool = editor.tool;
    let before_revision = session.revision;
    let result = match &target.key {
        SceneObjectKey::Water { kind, id, terrain, .. } =>
            crate::ui::water_feature_list::activate_listed_water_feature(editor, session,
                terrain.as_ref(), context.source_guard.busy(session), *kind, id, terrains, context.documents.as_deref_mut()),
        SceneObjectKey::Vegetation { terrain, .. } =>
            crate::ui::water_feature_list::focus_author_source(editor, session,
                terrain.as_ref(), context.source_guard.busy(session), terrains, context.documents.as_deref_mut()),
        _ => Ok(()),
    };
    if let Err(error) = result {
        editor.tool = previous_tool;
        editor.status = error;
        return false;
    }
    if session.revision != before_revision { context.source_guard.retarget_terrain_canvas(); }
    let Some(draft) = session.draft.as_ref() else {
        return false;
    };
    let (layer_target, index, tool) = match &target.key {
        SceneObjectKey::Water { kind, index, id, .. } => {
            if let Some(state) = context.water.as_mut() {
                state.clear_viewport_selection();
                state.confirm_viewport_feature(&draft.scene_id, *kind, id);
            }
            (
                LayerTarget::Water(*kind, id.clone()),
                Some(*index),
                editor.tool,
            )
        }
        SceneObjectKey::Vegetation { index, id, output, .. } => {
            if let Some(legacy) = legacy.as_deref_mut() { legacy.clear_vegetation_selection(); }
            if *output {
                editor.selected_vegetation_layer = *index;
                if let Some(legacy) = legacy {
                    legacy.selected_vegetation_scene_id = Some(draft.scene_id.clone());
                    legacy.selected_vegetation_output_id = Some(id.clone());
                }
            } else {
                crate::ui::actions::activate_vegetation_layer(
                    editor,
                    session,
                    *index,
                    context.profiles.as_deref_mut(),
                );
                editor.vegetation_layer_page =
                    *index / crate::ui::vegetation_layers::VEGETATION_LAYER_ROW_COUNT;
                editor.vegetation_preview_mode = VegetationPreviewMode::Source;
            }
            (
                LayerTarget::Vegetation(id.clone()),
                Some(*index),
                EditorTool::Vegetation,
            )
        }
        SceneObjectKey::Domain { kind, id, .. } => {
            let Some(state) = context.domain.as_mut() else { return false; };
            if state.snapshot_for(draft).is_none() { return false; }
            let valid = match kind {
                crate::game_authoring::scene_interaction::DomainItemKind::Layer => state.select_layer(id),
                crate::game_authoring::scene_interaction::DomainItemKind::Object => state.select_object(id),
            };
            if !valid { return false; }
            let info = state.selection_info(*kind, id);
            let selected = info.as_ref().and_then(|info| info.layer_id.clone()).unwrap_or_else(|| id.clone());
            (LayerTarget::Domain(selected), info.and_then(|info| info.layer_index), EditorTool::Domain(state.domain()))
        }
        _ => return false,
    };
    editor.tool = previous_tool; // 树直选只能选择，不能暗中打开笔刷或抢走左侧工具页。
    if let Some(modes) = context.modes.as_mut() {
        modes.remember_tool(tool);
    }
    let generation = context
        .domain
        .as_ref()
        .map_or(0, |state| state.observed_generation);
    let filter = context
        .modes
        .as_ref()
        .map_or_else(Default::default, |state| state.selection_filter(editor));
    if let Some(candidates) = candidates {
        candidates.confirm_tree_author(layer_target, index, session, generation, filter);
    }
    true
}
