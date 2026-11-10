//! 试听与滑杆共用当前预览身份、可见宿主和全局作者输入门。

use super::super::{
    AssetPreviewWorkspace,
    types::{AssetPreviewContent, AssetPreviewState},
};
use crate::{
    resources::*,
    ui::{sidebars::*, vegetation_preset_editor::VegetationPresetEditorState},
};
use bevy::{ecs::system::SystemParam, prelude::*, window::PrimaryWindow};

#[derive(SystemParam)]
pub(super) struct AudioContext<'w, 's> {
    pub editor: Res<'w, SceneEditorState>,
    pub sidebar: Res<'w, SceneEditorSidebarState>,
    pub workspace: Res<'w, VegetationPresetEditorState>,
    pub layout: Option<Res<'w, SceneEditorLayoutState>>,
    pub guard: Res<'w, SceneEditorNumericScrubMouseGuard>,
    pub numeric: Option<Res<'w, feathrise_editor_ui::ActiveNumericInputScrub>>,
    pub settings: Option<Res<'w, SceneEditorSettingsState>>,
    pub browser: Option<Res<'w, SceneBrowserState>>,
    pub deletion: Option<Res<'w, SceneDeletionTask>>,
    pub external: Option<Res<'w, crate::authoring::TerrainExternalChangeState>>,
    pub scene_drag: Option<Res<'w, SceneBrowserDragState>>,
    pub text: Option<Res<'w, EditorTextInputGuard>>,
    pub windows: Query<'w, 's, (Entity, &'static Window), With<PrimaryWindow>>,
}

impl AudioContext<'_, '_> {
    pub(super) fn owner(
        &self,
        preview: &AssetPreviewState,
        media: &AssetPreviewWorkspace,
    ) -> Option<Option<u64>> {
        if !self.editor.open
            || self.editor.close_requested
            || self.editor.new_scene_dialog_open
            || self.editor.pending_scene_transition.is_some()
            || self.editor.scene_switch_confirmation.is_some()
            || !matches!(preview.content, AssetPreviewContent::Audio { .. })
            || self.settings.as_ref().is_some_and(|state| state.open)
            || self
                .browser
                .as_ref()
                .is_some_and(|state| state.dialog.is_some())
            || self
                .deletion
                .as_ref()
                .is_some_and(|state| state.confirmation.is_some())
            || self
                .external
                .as_ref()
                .is_some_and(|state| state.modal_open())
            || self
                .windows
                .single()
                .ok()
                .is_some_and(|(_, window)| !window.focused)
        {
            return None;
        }
        if media.active(self.workspace.active_tab) {
            return media
                .expanded
                .as_ref()
                .filter(|tab| preview.selected_path.as_deref() == Some(tab.path.as_str()))
                .map(|tab| Some(tab.id));
        }
        (self.sidebar.right == EditorRightSidebarTab::Assets
            && self
                .layout
                .as_ref()
                .is_none_or(|state| state.inspector_shown())
            && preview.selected_path == self.editor.selected_asset)
            .then_some(None)
    }
}
