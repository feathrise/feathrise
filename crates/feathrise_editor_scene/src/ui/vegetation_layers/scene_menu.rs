//! 统一树植被命令只改冻结的作者层，复用原Arc密度快照、编译门与撤销事务。

use crate::{
    authoring::{EditorAuthoringSession, VegetationBrushStroke},
    resources::SceneEditorState,
    types::EditorAction,
    ui::scene_context_menu::{SceneContextMenuState, SceneMenuAction, SceneObjectKey},
};
use bevy::prelude::*;

pub(crate) fn input(
    menu: Res<SceneContextMenuState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut editor: ResMut<SceneEditorState>,
    stroke: Res<VegetationBrushStroke>,
) {
    let Some(request) = menu.ready.as_ref().filter(|request| {
        !stroke.active
            && request.target.current(&editor, &session)
            && request.target.allows(request.action)
    }) else {
        return;
    };
    let SceneObjectKey::Vegetation {
        index,
        output: false,
        ..
    } = &request.target.key
    else {
        return;
    };
    let action = match request.action {
        SceneMenuAction::Copy => EditorAction::VegetationDuplicateLayer,
        SceneMenuAction::Delete => EditorAction::VegetationDeleteSelected,
        SceneMenuAction::Visibility => EditorAction::VegetationToggleEnabled,
        SceneMenuAction::Lock => EditorAction::VegetationToggleLocked,
        _ => return,
    };
    editor.selected_vegetation_layer = *index;
    match super::operations::apply_vegetation_authoring_action(
        action,
        &mut editor,
        &mut session,
        None,
    ) {
        Ok(outcome) => editor.status = outcome.status,
        Err(error) => editor.status = error,
    }
}
