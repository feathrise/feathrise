//! 场景网格右键复用原结构事务，显隐和锁定继续使用当前编辑会话的集合。

use super::{components::Row, resources::TerrainObjectsEditorState, systems, types::Mutation};
use crate::{
    authoring::EditorAuthoringSession,
    resources::SceneEditorState,
    ui::scene_context_menu::{
        SceneContextMenuState, SceneMenuAction, SceneObjectKey, SceneObjectRow, SceneObjectTarget,
    },
};
use bevy::prelude::*;

pub(super) fn bind_rows(
    mut commands: Commands,
    session: Res<EditorAuthoringSession>,
    state: Res<TerrainObjectsEditorState>,
    rows: Query<(Entity, &Row, Option<&SceneObjectRow>)>,
) {
    for (entity, row, previous) in &rows {
        let target = session.draft.as_ref().and_then(|draft| {
            let index = state.list_start + row.0;
            let objects = draft
                .manifest
                .terrains
                .as_deref()
                .unwrap_or_else(|| state.legacy.as_deref().map_or(&[], std::slice::from_ref));
            let object = objects.get(index)?;
            Some(SceneObjectTarget {
                scene_id: draft.scene_id.clone(),
                revision: session.revision,
                key: SceneObjectKey::Terrain {
                    index,
                    id: object.id.clone(),
                },
                name: object.display_name.clone(),
                hidden: state.hidden.contains(&object.id),
                locked: state.locked.contains(&object.id),
                copyable: true,
            })
        });
        if previous.is_none_or(|previous| previous.0 != target) {
            commands.entity(entity).insert(SceneObjectRow(target));
        }
    }
}

pub(super) fn input(
    menu: Option<Res<SceneContextMenuState>>,
    mut editor: ResMut<SceneEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut state: ResMut<TerrainObjectsEditorState>,
    mut visibility: ResMut<feathrise_scene::TerrainObjectEditorVisibility>,
) {
    let Some(request) = menu.as_ref().and_then(|menu| menu.ready.as_ref()) else {
        return;
    };
    let SceneObjectKey::Terrain { id, .. } = &request.target.key else {
        return;
    };
    if !request.target.current(&editor, &session)
        || !request.target.allows(request.action)
        || !systems::enabled(&editor, &session, &state)
    {
        return;
    }
    match request.action {
        SceneMenuAction::Rename => systems::start(
            &mut session,
            &mut editor,
            &mut state,
            Mutation::Rename(id.clone(), request.name.clone()),
        ),
        SceneMenuAction::Delete => systems::start(
            &mut session,
            &mut editor,
            &mut state,
            Mutation::Remove(id.clone()),
        ),
        SceneMenuAction::Visibility => {
            if request.target.hidden {
                state.hidden.remove(id);
                std::sync::Arc::make_mut(&mut editor.terrain_hidden).remove(id);
                visibility.hidden.remove(id);
            } else {
                state.hidden.insert(id.clone());
                std::sync::Arc::make_mut(&mut editor.terrain_hidden).insert(id.clone());
                visibility.hidden.insert(id.clone());
            }
            editor.status = if request.target.hidden {
                "已恢复网格在编辑器中的显示"
            } else {
                "已隐藏编辑器网格；场景参与状态和碰撞保持有效"
            }
            .into();
        }
        SceneMenuAction::Lock => {
            if request.target.locked {
                state.locked.remove(id);
                editor.terrain_locked.remove(id);
            } else {
                state.locked.insert(id.clone());
                editor.terrain_locked.insert(id.clone());
            }
            editor.status = if request.target.locked {
                "已解锁网格"
            } else {
                "网格已锁定，可以选择查看"
            }
            .into();
        }
        _ => {}
    }
}
