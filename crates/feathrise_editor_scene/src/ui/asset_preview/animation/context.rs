//! 动画工作区沿用文件预览宿主及F12模态门，防止隐藏页和失焦窗口继续编辑。

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
pub(super) struct AnimationContext<'w, 's> {
    pub editor: Res<'w, SceneEditorState>,
    pub sidebar: Res<'w, SceneEditorSidebarState>,
    pub workspace: Res<'w, VegetationPresetEditorState>,
    pub layout: Option<Res<'w, SceneEditorLayoutState>>,
    pub guard: Res<'w, SceneEditorNumericScrubMouseGuard>,
    pub numeric: Option<Res<'w, feathrise_editor_ui::ActiveNumericInputScrub>>,
    pub numeric_text: Option<Res<'w, feathrise_editor_ui::ActiveNumericInput>>,
    pub settings: Option<Res<'w, SceneEditorSettingsState>>,
    pub browser: Option<Res<'w, SceneBrowserState>>,
    pub deletion: Option<Res<'w, SceneDeletionTask>>,
    pub external: Option<Res<'w, crate::authoring::TerrainExternalChangeState>>,
    pub scene_drag: Option<Res<'w, SceneBrowserDragState>>,
    pub text: Option<Res<'w, EditorTextInputGuard>>,
    focus: Option<Res<'w, bevy::input_focus::InputFocus>>,
    text_fields: Query<
        'w,
        's,
        (),
        Or<(With<crate::components::EditorTextInput>, With<bevy::text::EditableText>)>,
    >,
    pub windows: Query<'w, 's, (Entity, &'static Window), With<PrimaryWindow>>,
}

impl AnimationContext<'_, '_> {
    /// 普通文字焦点也独占快捷键；IME保护只表示组合态，不能替代此检查或禁止鼠标切焦。
    pub(super) fn text_input_focused(&self) -> bool {
        self.focus
            .as_ref()
            .and_then(|focus| focus.get())
            .is_some_and(|entity| self.text_fields.contains(entity))
    }

    pub(super) fn available(
        &self,
        preview: &AssetPreviewState,
        media: &AssetPreviewWorkspace,
    ) -> bool {
        if !self.editor.open
            || self.editor.close_requested
            || self.editor.new_scene_dialog_open
            || self.editor.pending_scene_transition.is_some()
            || self.editor.scene_switch_confirmation.is_some()
            || !matches!(preview.content, AssetPreviewContent::Model { .. })
            || preview.external_preview.is_some()
            || self.settings.as_ref().is_some_and(|s| s.open)
            || self.browser.as_ref().is_some_and(|s| s.dialog.is_some())
            || self
                .deletion
                .as_ref()
                .is_some_and(|s| s.confirmation.is_some())
            || self.external.as_ref().is_some_and(|s| s.modal_open())
            || self
                .windows
                .single()
                .ok()
                .is_some_and(|(_, window)| !window.focused)
        {
            return false;
        }
        if media.active(self.workspace.active_tab) {
            return media
                .expanded
                .as_ref()
                .is_some_and(|tab| preview.selected_path.as_deref() == Some(tab.path.as_str()));
        }
        self.sidebar.right == EditorRightSidebarTab::Assets
            && self
                .layout
                .as_ref()
                .is_none_or(|layout| layout.inspector_shown())
            && preview.selected_path == self.editor.selected_asset
    }
}
