//! 从真实世界、当前草稿与已确认选择构造O(1)资格，不读取文件或解析对象几何。

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use feathrise_scene_content::SceneTerrainKind;

use super::EditorActionAvailability;
use crate::authoring::{
    EditorAuthoringSession, TerrainExternalChangeState, TerrainPreviewState, WaterNetworkEditState,
};
use crate::resources::{
    SceneBrowserState, SceneDeletionTask, SceneEditorNumericScrubMouseGuard, SceneEditorSettingsState,
    SceneEditorState, SceneEditorViewportState, SceneEditorWorldSession,
};
use crate::types::EditorTool;
use crate::ui::gltf_browser::EditorGltfBrowserState;
use crate::ui::vegetation_preset_editor::{PresetWorkspaceTab, VegetationPresetEditorState};
use crate::viewport::{LayerCandidateState, LayerTarget};

/// 不借用编辑器或视口资源，让输入入口可在同帧修改它们后重新判断，避免ECS读写冲突。
#[derive(SystemParam)]
pub(crate) struct EditorActionInputs<'w> {
    session: Res<'w, EditorAuthoringSession>,
    world: SceneEditorWorldSession<'w>,
    terrain: Option<Res<'w, TerrainPreviewState>>,
    workspace: Option<Res<'w, VegetationPresetEditorState>>,
    browser: Option<Res<'w, EditorGltfBrowserState>>,
    candidates: Option<Res<'w, LayerCandidateState>>,
    deletion: Option<Res<'w, SceneDeletionTask>>,
    guard: Option<Res<'w, SceneEditorNumericScrubMouseGuard>>,
    water_edit: Option<Res<'w, WaterNetworkEditState>>,
    settings_modal: Option<Res<'w, SceneEditorSettingsState>>,
    folders: Option<Res<'w, SceneBrowserState>>,
    conflict: Option<Res<'w, TerrainExternalChangeState>>,
    terrain_objects: Option<Res<'w, crate::ui::terrain_objects::TerrainObjectsEditorState>>,
    mesh_edit: Option<Res<'w, crate::authoring::mesh_edit::MeshEditState>>,
}

impl EditorActionInputs<'_> {
    pub(in crate::ui) fn snapshot(
        &self,
        editor: &SceneEditorState,
        viewport: Option<&SceneEditorViewportState>,
    ) -> EditorActionAvailability {
        let draft = self
            .session
            .draft
            .as_ref()
            .filter(|draft| editor.selected_scene_id.as_ref() == Some(&draft.scene_id));
        EditorActionAvailability {
            blocked: !editor.open
                || self.mesh_edit.as_ref().is_some_and(|state| state.moving())
                || self.terrain_objects.as_ref().is_some_and(|state| state.busy())
                || editor.close_requested
                || editor.new_scene_dialog_open
                || editor.scene_transition_in_progress
                || editor.pending_scene_transition.is_some()
                || editor.scene_switch_confirmation.is_some()
                || self.guard.as_ref().is_some_and(|guard| guard.blocked())
                || self.water_edit.as_ref().is_some_and(|edit| edit.dragging())
                || self
                    .settings_modal
                    .as_ref()
                    .is_some_and(|state| state.open || state.owns_mouse_this_frame)
                || self
                    .folders
                    .as_ref()
                    .is_some_and(|state| state.dialog.is_some())
                || self
                    .conflict
                    .as_ref()
                    .is_some_and(|state| state.modal_open()),
            other_workspace: self
                .workspace
                .as_ref()
                .is_some_and(|workspace| workspace.active_tab != PresetWorkspaceTab::Scene),
            mesh_editing: self.mesh_edit.as_ref().is_some_and(|state| state.active()),
            current_scene: self.world.ready() && draft.is_some(),
            viewport_ready: viewport.is_some_and(|viewport| {
                draft.is_some_and(|draft| viewport.framed_scene_id.as_ref() == Some(&draft.scene_id))
            }),
            saving: self.session.saving,
            undo_empty: self.session.undo.is_empty(),
            redo_empty: self.session.redo.is_empty(),
            terrain_pending: self
                .terrain
                .as_ref()
                .is_some_and(|terrain| dynamic_terrain_save_is_pending(&self.session, terrain)),
            deletion_busy: self.deletion.as_ref().is_some_and(|state| state.busy()),
            focus_available: self.focus_available(editor),
        }
    }

    /// 复用取景入口的选择分支：只看已确认对象与O(1)层下标，悬停候选不算选择。
    fn focus_available(&self, editor: &SceneEditorState) -> bool {
        if self.candidates.as_ref().is_some_and(|candidates|matches!(candidates.selected,Some(LayerTarget::TerrainObject(_))))
            && self.terrain_objects.as_ref().is_some_and(|state|crate::ui::terrain_objects::selected(&self.session,state).is_some()) { return true; }
        if editor.tool == EditorTool::Inspect
            && let Some(candidates) = &self.candidates
            && candidates.has_viewport_selection()
        {
            return matches!(
                candidates.selected,
                Some(LayerTarget::Placement(_))
                    | Some(LayerTarget::Water(..))
                    | Some(LayerTarget::Portal(_))
            );
        }
        match editor.tool {
            EditorTool::Inspect | EditorTool::PlaceAsset => {
                self.browser.as_ref().is_some_and(|browser| {
                    browser
                        .selection
                        .as_ref()
                        .map_or(browser.selected_placement_id.is_some(), |selection| {
                            selection.valid && !selection.orphaned_water_binding
                        })
                })
            }
            EditorTool::Portal => editor.selected_current_portal_id.is_some(),
            EditorTool::Water | EditorTool::RiverSpline | EditorTool::Waterfall => self
                .session
                .draft
                .as_ref()
                .and_then(|draft| draft.water.as_ref())
                .is_some_and(|water| match editor.tool {
                    EditorTool::Water => water.layers.get(editor.selected_water_layer).is_some(),
                    EditorTool::RiverSpline => {
                        water.rivers.get(editor.selected_water_river).is_some()
                    }
                    EditorTool::Waterfall => {
                        water.waterfalls.get(editor.selected_waterfall).is_some()
                    }
                    _ => false,
                }),
            _ => false,
        }
    }
}

pub(in crate::ui) fn dynamic_terrain_save_is_pending(
    session: &EditorAuthoringSession,
    terrain_preview: &TerrainPreviewState,
) -> bool {
    session.draft.as_ref().is_some_and(|draft| {
        draft.terrain_dirty
            && draft.manifest.is_data_driven()
            && draft.manifest.terrain.kind == SceneTerrainKind::Heightfield
            && !terrain_preview.ready_for(&draft.scene_id, session.terrain_revision)
    }) // 同修订Mesh、Trimesh和内存高度全部完成，人工保存才恢复资格。
}
