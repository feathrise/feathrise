//! 显式网格植被等待起笔：只冻结弱地面票据和作者层身份，不持有或复制作者像素。
use crate::{
    authoring::{
        EditorAuthoringSession, terrain_documents,
        terrain_ground::{CursorGroundSnapshot, CursorIdentity, TerrainGroundCursor},
    },
    resources::SceneEditorState,
    types::EditorTool,
};
use bevy::prelude::*;
use feathrise_scene_content::TerrainId;

#[derive(Clone, PartialEq)]
pub(crate) struct ObjectVegetationPress {
    cursor: CursorIdentity,
    input_authority: u64,
    input_epoch: u64,
    terrain: TerrainId,
    vegetation_revision: u64,
    layer: (usize, String),
    erase: bool,
}

fn capture(
    editor: &SceneEditorState,
    session: &EditorAuthoringSession,
    cursor: &TerrainGroundCursor,
    erase: bool,
) -> Option<ObjectVegetationPress> {
    let draft = session.draft.as_ref()?;
    let active = draft.terrain_documents.active.as_ref()?;
    let identity = cursor.identity.as_ref()?;
    let snapshot = draft.terrain_ground.as_ref()?;
    let layer = draft
        .vegetation
        .as_ref()?
        .authoring
        .as_ref()?
        .layers
        .get(editor.selected_vegetation_layer)?;
    if !terrain_documents::canvas_writable(session, editor)
        || !layer.enabled
        || layer.locked
        || editor.selected_scene_id.as_ref() != Some(&draft.scene_id)
        || identity.scene_id != draft.scene_id
        || identity.revision != session.revision
        || identity.terrain_revision != session.terrain_revision
        || identity.tool != EditorTool::Vegetation
        || snapshot.scene_id != draft.scene_id
        || snapshot.revision != session.terrain_revision
        || identity.ground_snapshot.as_ref() != Some(&CursorGroundSnapshot::capture(snapshot))
    {
        return None;
    }
    Some(ObjectVegetationPress {
        cursor: identity.clone(),
        input_authority: cursor.input_authority()?,
        input_epoch: cursor.ray?.epoch,
        terrain: active.definition.id.clone(),
        vegetation_revision: session.vegetation_revision,
        layer: (editor.selected_vegetation_layer, layer.id.clone()),
        erase,
    })
}

pub(super) fn update(
    pending: &mut Option<ObjectVegetationPress>,
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
        *pending = None; // 松手、失焦、换层或画布换代永久取消；恢复旧状态不能继承之前的按压。
    }
    if mouse.just_pressed(MouseButton::Left) {
        *pending = current;
        if pending.is_some() && editor.brush_cursor_world.is_none() {
            editor.status = "正在等待所选网格的真实表面；按住继续刷草，松开取消起笔".into();
        }
    }
}

pub(super) fn active_is_current(
    active: &ObjectVegetationPress,
    editor: &SceneEditorState,
    session: &EditorAuthoringSession,
    cursor: Option<&TerrainGroundCursor>,
) -> bool {
    let Some(current) = cursor.and_then(|cursor| capture(editor, session, cursor, active.erase)) else {
        return false;
    };
    let mut expected = active.clone();
    // 同一笔会推进作者修订；相机、来源、作者层和按压所有权仍必须与按下时完全一致。
    expected.cursor.revision = session.revision;
    expected.vegetation_revision = session.vegetation_revision;
    expected == current
}

pub(super) fn author_is_current(active: &ObjectVegetationPress, editor: &SceneEditorState,
    session: &EditorAuthoringSession) -> bool {
    let Some(draft) = session.draft.as_ref() else { return false; };
    editor.selected_scene_id.as_ref() == Some(&active.cursor.scene_id)
        && draft.scene_id == active.cursor.scene_id && terrain_documents::canvas_writable(session, editor)
        && draft.terrain_documents.active.as_ref().is_some_and(|canvas| canvas.definition.id == active.terrain)
        && editor.selected_vegetation_layer == active.layer.0
        && draft.vegetation.as_ref().and_then(|vegetation| vegetation.authoring.as_ref())
            .and_then(|authoring| authoring.layers.get(active.layer.0))
            .is_some_and(|layer| layer.id == active.layer.1 && layer.enabled && !layer.locked)
}
