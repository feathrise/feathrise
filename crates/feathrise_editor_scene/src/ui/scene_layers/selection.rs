//! 新增作者行与右键菜单共用同一选择桥，保留选择模式并恢复对应的下一次编辑工具。

use crate::{
    authoring::EditorAuthoringSession, game_authoring::scene_interaction::SceneInteractionState,
    resources::{EditorViewportSelectionState, SceneEditorState},
    types::{EditorTool, VegetationPreviewMode},
    ui::scene_context_menu::{SceneObjectKey, SceneObjectTarget},
    viewport::{
        LayerCandidateState, LayerTarget, SelectedWaterViewportState, modes::ViewportModeState,
    },
};
use bevy::{ecs::system::SystemParam, prelude::*};

#[derive(SystemParam)]
pub(crate) struct LayerSelectionContext<'w> {
    pub domain: Option<ResMut<'w, SceneInteractionState>>,
    pub modes: Option<ResMut<'w, ViewportModeState>>,
    water: Option<ResMut<'w, SelectedWaterViewportState>>,
    profiles: Option<ResMut<'w, feathrise_scene::DynamicMeshProfileRequests>>,
    sidebar: Option<ResMut<'w, crate::ui::sidebars::SceneEditorSidebarState>>,
    documents: Option<ResMut<'w, crate::authoring::terrain_documents::TerrainDocumentTask>>,
    source_guard: crate::ui::water_feature_list::WaterSourceSelectionGuard<'w>,
}

impl LayerSelectionContext<'_> {
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
