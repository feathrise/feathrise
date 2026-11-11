//! 静水平面按压身份：冻结源画布与水位，失去输入所有权、换源或外部修改后不得复活旧按压。
use crate::{
    authoring::{EditorAuthoringSession, water_canvas},
    resources::SceneEditorState,
    types::EditorTool,
    viewport::water_plane::{WaterPlaneCursor, WaterPlaneIdentity},
};
use bevy::prelude::*;

#[derive(Clone, PartialEq)]
pub(crate) struct ObjectWaterPress {
    cursor: WaterPlaneIdentity,
    authority: u64,
    epoch: u64,
    revision: u64,
    water_revision: u64,
}

fn capture(editor: &SceneEditorState, session: &EditorAuthoringSession,
    cursor: &WaterPlaneCursor) -> Option<ObjectWaterPress> {
    let draft = session.draft.as_ref()?;
    let identity = cursor.identity.as_ref()?;
    let layer = draft.water.as_ref()?.layers.get(editor.selected_water_layer)?;
    if !water_canvas::writable(session, editor) || !cursor.available
        || editor.selected_scene_id.as_ref() != Some(&draft.scene_id)
        || editor.tool != EditorTool::Water || identity.canvas != water_canvas::identity(draft)
        || identity.layer != (editor.selected_water_layer, layer.id.clone(), layer.water_level.to_bits()) {
        return None;
    }
    Some(ObjectWaterPress {
        cursor: identity.clone(), authority: cursor.authority, epoch: cursor.epoch,
        revision: session.revision, water_revision: session.water_revision,
    })
}

pub(super) fn update(pending: &mut Option<ObjectWaterPress>, available: bool,
    editor: &SceneEditorState, session: &EditorAuthoringSession,
    cursor: Option<&WaterPlaneCursor>, mouse: &ButtonInput<MouseButton>) {
    let current = (available && (mouse.pressed(MouseButton::Left) || mouse.just_pressed(MouseButton::Left)))
        .then(|| cursor.and_then(|cursor| capture(editor, session, cursor))).flatten();
    if pending.as_ref() != current.as_ref() { *pending = None; }
    if mouse.just_pressed(MouseButton::Left) { *pending = current; }
}

pub(super) fn active_is_current(press: &ObjectWaterPress, editor: &SceneEditorState,
    session: &EditorAuthoringSession, cursor: Option<&WaterPlaneCursor>) -> bool {
    cursor.and_then(|cursor| capture(editor, session, cursor)).as_ref() == Some(press)
}

pub(super) fn origin_is_current(press: &ObjectWaterPress, session: &EditorAuthoringSession) -> bool {
    // 模态/失焦只排空已受理段；外部作者换代则不能把旧规格的冻结段解释为新画布。
    press.revision == session.revision && press.water_revision == session.water_revision
        && session.draft.as_ref().is_some_and(|draft| water_canvas::identity(draft) == press.cursor.canvas)
}

pub(super) fn record_revision(press: &mut Option<ObjectWaterPress>, session: &EditorAuthoringSession) {
    if let Some(press) = press {
        // 只在本消费者真正处理冻结段后推进预期作者代次；其他修改不能被稳定按住悄悄继承。
        press.revision = session.revision;
        press.water_revision = session.water_revision;
    }
}
