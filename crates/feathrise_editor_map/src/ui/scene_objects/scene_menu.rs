//! 对象菜单的名称、显隐及锁定进入原单对象管理事务，不借当前多选或组名。

use super::{
    SceneObjectState,
    components::ObjectAction,
    resources::{ObjectTargets, PendingManagement},
};
use crate::{
    authoring::EditorAuthoringSession,
    resources::MapEditorState,
    ui::scene_context_menu::{SceneContextMenuState, SceneMenuAction, SceneObjectKey},
};
use bevy::prelude::*;

pub(super) fn input(
    menu: Option<Res<SceneContextMenuState>>,
    editor: Res<MapEditorState>,
    session: Res<EditorAuthoringSession>,
    mut state: ResMut<SceneObjectState>,
) {
    let Some(request) = menu.as_ref().and_then(|menu| menu.ready.as_ref()) else {
        return;
    };
    let SceneObjectKey::Placement { index, .. } = &request.target.key else {
        return;
    };
    let action = match request.action {
        SceneMenuAction::Rename => ObjectAction::Rename,
        SceneMenuAction::Visibility => ObjectAction::ToggleHidden,
        SceneMenuAction::Lock => ObjectAction::ToggleLocked,
        _ => return,
    };
    if !request.target.current(&editor, &session)
        || !request.target.allows(request.action)
        || state.busy()
        || state.retirement.is_some()
    {
        return;
    }
    state.pending = Some(PendingManagement {
        map: request.target.map.clone(),
        revision: session.revision,
        structure: session.placement_structure_revision,
        terrain_revision: session.terrain_revision,
        derived_pose_revision: session.placement_derived_pose_revision,
        targets: ObjectTargets::Indices(vec![*index]),
        action,
        name: request.name.clone(),
        group: String::new(),
        flag: if request.action == SceneMenuAction::Visibility {
            !request.target.hidden
        } else {
            !request.target.locked
        },
        flag_set: true,
        cursor: 0,
        changes: Vec::new(),
        heap_bytes: 0,
        selected: Vec::new(),
    });
}
