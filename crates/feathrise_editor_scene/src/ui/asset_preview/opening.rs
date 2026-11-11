//! 资产列表与缩略图共享双击打开入口；本帧输入先确认，随后按文件类型激活独立中央标签。

use super::{messages::OpenAssetPreview, resources::*, types::*, workspace, DocumentState};
use crate::resources::{EditorTextInputGuard, SceneEditorLayoutState, SceneEditorNumericScrubMouseGuard, SceneEditorState};
use crate::ui::{sidebars::{EditorRightSidebarTab, SceneEditorSidebarState}, vegetation_preset_editor::VegetationPresetEditorState};
use bevy::{prelude::*, text::EditableText};

#[cfg(test)]
#[path = "opening_tests.rs"]
mod tests;

pub(super) fn open(
    mut requests: MessageReader<OpenAssetPreview>,
    mut editor: ResMut<SceneEditorState>,
    sidebar: Res<SceneEditorSidebarState>,
    layout: Option<Res<SceneEditorLayoutState>>,
    guard: Res<EditorTextInputGuard>,
    mouse: Res<SceneEditorNumericScrubMouseGuard>,
    mut documents: ResMut<DocumentState>,
    mut media: ResMut<AssetPreviewWorkspace>,
    mut workspace: ResMut<VegetationPresetEditorState>,
    state: Res<AssetPreviewState>,
    navigation: Res<AssetPreviewNavigation>,
    inputs: Query<&EditableText>,
    modals: (
        Option<Res<crate::resources::SceneEditorSettingsState>>,
        Option<Res<crate::resources::SceneBrowserState>>,
        Option<Res<crate::resources::SceneDeletionTask>>,
        Option<Res<crate::authoring::TerrainExternalChangeState>>,
    ),
) {
    let Some(request) = requests.read().last().cloned() else { return; }; // 合并同帧边沿，阻挡后的请求不补执行。
    let (settings, browser, deletion, conflict) = modals;
    if !editor.open || editor.close_requested || editor.new_scene_dialog_open
        || editor.pending_scene_transition.is_some() || sidebar.right != EditorRightSidebarTab::Assets
        || layout.is_some_and(|layout| !layout.inspector_shown())
        || guard.block_shortcuts || mouse.blocked()
        || settings.is_some_and(|state| state.open) || browser.is_some_and(|state| state.dialog.is_some())
        || deletion.is_some_and(|state| state.confirmation.is_some()) || conflict.is_some_and(|state| state.modal_open())
        || inputs.iter().any(|input| input.is_composing() || input.pending_paste.is_some() || !input.pending_edits.is_empty())
        || editor.selected_asset.as_deref() != Some(request.path.as_str())
    { return; }
    let result = if crate::asset_catalog::document_format(&request.path).is_some() {
        documents.expand(request.path, &mut workspace)
    } else if workspace::media_path(&request.path) {
        workspace::expand(&mut media, &mut workspace, &request.path, &state, &navigation)
    } else { return; };
    if let Err(error) = result { editor.status = error; }
}
