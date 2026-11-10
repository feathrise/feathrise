//! 已采用普通资产选集收口其他类别的旧高亮；只在成员变化边沿执行，不改作者层记忆。

use super::PlacementSelectionSet;
use crate::{
    authoring::BoundaryEditState, game_authoring::scene_interaction::SceneInteractionState,
    resources::{EditorViewportSelectionState, SceneEditorState},
    types::EditorTool,
    viewport::{
        SelectedPlacementViewportState, SelectedPortalViewportState, SelectedWaterViewportState,
        modes::ViewportModeState,
    },
};
use bevy::prelude::*;

pub(crate) fn sync_multi_selection_ownership(
    selection: Res<PlacementSelectionSet>,
    mut editor: ResMut<SceneEditorState>,
    mut legacy: ResMut<EditorViewportSelectionState>,
    mut characters: ResMut<SceneInteractionState>,
    mut water: ResMut<SelectedWaterViewportState>,
    mut portal: ResMut<SelectedPortalViewportState>,
    mut boundary: ResMut<BoundaryEditState>,
    mut placement: ResMut<SelectedPlacementViewportState>,
    mut modes: ResMut<ViewportModeState>,
) {
    if !selection.is_changed()
        || selection.members.is_empty()
        || !editor.open
        || selection.scene_id != editor.selected_scene_id
    {
        return;
    }
    editor.selected_current_portal_id = None;
    legacy.clear_vegetation_selection();
    water.clear_viewport_selection();
    portal.clear_click_sequence();
    boundary.clear_drag();
    placement.clear_click_sequence();
    characters.hover_layer = None;
    characters.suppress_layer_highlight = true;
    modes.remember_tool(EditorTool::Inspect);
}
