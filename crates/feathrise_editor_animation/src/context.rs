//! 动画工作区沿用文件预览宿主及F12模态门，防止隐藏页和失焦窗口继续编辑。

use feathrise_editor_animation_model::{
    host::{AnimationHost, SceneEditorNumericScrubMouseGuard},
    media::{AssetPreviewContent, AssetPreviewState, AssetPreviewWorkspace},
};
use feathrise_editor_core::{EditorTextInput, EditorTextInputGuard};
use bevy::{ecs::system::SystemParam, prelude::*, window::PrimaryWindow};
use std::path::Path;

#[derive(SystemParam)]
pub(super) struct AnimationContext<'w, 's> {
    pub host: Res<'w, AnimationHost>,
    pub guard: Res<'w, SceneEditorNumericScrubMouseGuard>,
    pub numeric: Option<Res<'w, feathrise_editor_ui::ActiveNumericInputScrub>>,
    pub numeric_text: Option<Res<'w, feathrise_editor_ui::ActiveNumericInput>>,
    pub text: Option<Res<'w, EditorTextInputGuard>>,
    focus: Option<Res<'w, bevy::input_focus::InputFocus>>,
    text_fields: Query<
        'w,
        's,
        (),
        Or<(With<EditorTextInput>, With<bevy::text::EditableText>)>,
    >,
    pub windows: Query<'w, 's, (Entity, &'static Window), With<PrimaryWindow>>,
}

impl AnimationContext<'_, '_> {
    pub(super) fn source_matches(
        &self,
        editor: &super::resources::AnimationEditorState,
        preview: &AssetPreviewState,
        media: &AssetPreviewWorkspace,
        asset_root: &Path,
    ) -> bool {
        if let Some(request) = &editor.rig_preview {
            return request.request.matches(preview, media, self.host.tab, asset_root);
        }
        preview.selected_path.as_ref().is_some_and(|path| {
            editor.source() == Some(asset_root.join(path).as_path())
        })
    }

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
        if !self.host.open
            || self.host.closing
            || self.host.switching_scene
            || self.host.modal_open
            || !matches!(preview.content, AssetPreviewContent::Model { .. })
            || preview.external_preview.is_some()
            || self
                .windows
                .single()
                .ok()
                .is_some_and(|(_, window)| !window.focused)
        {
            return false;
        }
        if media.active(self.host.tab) {
            return media
                .expanded
                .as_ref()
                .is_some_and(|tab| preview.selected_path.as_deref() == Some(tab.path.as_str()));
        }
        self.host.sidebar_assets_visible && preview.selected_path == self.host.selected_asset
    }
}
