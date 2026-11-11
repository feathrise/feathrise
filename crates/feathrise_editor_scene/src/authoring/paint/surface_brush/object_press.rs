//! 显式网格地表起笔等待：按下先冻结权限，真实射线完成后才由原笔刷建立单笔事务。

use super::types::ObjectSurfacePress;
use crate::{
    authoring::{
        EditorAuthoringSession, terrain_documents,
        terrain_ground::{CursorGroundSnapshot, TerrainGroundCursor},
    },
    resources::SceneEditorState,
    types::EditorTool,
};
use bevy::prelude::*;

fn capture(
    editor: &SceneEditorState,
    session: &EditorAuthoringSession,
    cursor: &TerrainGroundCursor,
    erase: bool,
) -> Option<ObjectSurfacePress> {
    let draft = session.draft.as_ref()?;
    let active = draft.terrain_documents.active.as_ref()?;
    let identity = cursor.identity.as_ref()?;
    let snapshot = draft.terrain_ground.as_ref()?;
    if !terrain_documents::canvas_writable(session, editor)
        || editor.selected_scene_id.as_ref() != Some(&draft.scene_id)
        || identity.scene_id != draft.scene_id
        || identity.revision != session.revision
        || identity.terrain_revision != session.terrain_revision
        || identity.tool != EditorTool::Surface
        || snapshot.scene_id != draft.scene_id
        || snapshot.revision != session.terrain_revision
        || identity.ground_snapshot.as_ref() != Some(&CursorGroundSnapshot::capture(snapshot))
    {
        return None;
    }
    Some(ObjectSurfacePress {
        cursor: identity.clone(),
        input_authority: cursor.input_authority()?,
        input_epoch: cursor.ray?.epoch,
        terrain: active.definition.id.clone(),
        surface_revision: session.surface_revision,
        layer: draft
            .surface
            .as_ref()
            .and_then(|surface| surface.layers.get(editor.selected_surface_layer))
            .map(|layer| (editor.selected_surface_layer, layer.id.clone())),
        preset: editor.surface_selected_preset_id.clone(),
        erase,
    })
}

pub(super) fn update(
    pending: &mut Option<ObjectSurfacePress>,
    available: bool,
    editor: &mut SceneEditorState,
    session: &EditorAuthoringSession,
    cursor: Option<&TerrainGroundCursor>,
    mouse: &ButtonInput<MouseButton>,
    erase: bool,
) {
    let current = (available && mouse.pressed(MouseButton::Left))
        .then(|| cursor.and_then(|cursor| capture(editor, session, cursor, erase)))
        .flatten();
    if pending.as_ref() != current.as_ref() {
        *pending = None; // 松手或任一身份变化永久撤销；恢复旧选择不能继承之前的按压。
    }
    if mouse.just_pressed(MouseButton::Left) {
        *pending = current;
        if pending.is_some() && editor.brush_cursor_world.is_none() {
            editor.status = "正在等待所选网格的真实表面；按住继续绘制，松开取消起笔".into();
        }
    }
}

pub(super) fn active_is_current(
    active: &ObjectSurfacePress,
    editor: &SceneEditorState,
    session: &EditorAuthoringSession,
    cursor: Option<&TerrainGroundCursor>,
) -> bool {
    let Some(current) = cursor.and_then(|cursor| capture(editor, session, cursor, active.erase)) else { return false; };
    let mut expected = active.clone();
    // 本笔修改会推进这两个版本；其他来源、相机、按压、图层和鼠标所有权仍须完全一致。
    expected.cursor.revision = session.revision;
    expected.surface_revision = session.surface_revision;
    expected == current
}
