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

impl ObjectSurfacePress {
    fn matches(&self, current: &Self) -> bool {
        let mut expected = self.clone();
        // 作者页或兄弟对象采用可重建全场查询；按压只继承同一目标的几何和姿态。
        // 最新快照与真实最近射线仍由光标完成门核对，绝不放行旧命中。
        expected.cursor.ground_snapshot = current.cursor.ground_snapshot.clone();
        expected == *current
    }
}

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
        surface: snapshot.object_paint_identity(&active.definition.id)?,
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
    if pending.as_ref().is_some_and(|previous| current.as_ref().is_none_or(|current| !previous.matches(current))) {
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
    let current = cursor.and_then(|cursor| capture(editor, session, cursor, active.erase));
    let mut expected = active.clone();
    // 本笔修改会推进这两个版本；其他来源、相机、按压、图层和鼠标所有权仍须完全一致。
    expected.cursor.revision = session.revision;
    expected.surface_revision = session.surface_revision;
    let valid = current.as_ref().is_some_and(|current| expected.matches(current));
    #[cfg(feature = "validation")]
    if !valid {
        // 调用方同帧收口，本笔只记录一次固定大小失效字段；诊断不能延长查询快照寿命或恢复权限。
        eprintln!("地表活笔取消：{}", cancellation(&expected, current.as_ref(), editor, session, cursor));
    }
    valid
}

#[cfg(feature = "validation")]
fn cancellation(previous: &ObjectSurfacePress, current: Option<&ObjectSurfacePress>,
    editor: &SceneEditorState, session: &EditorAuthoringSession, cursor: Option<&TerrainGroundCursor>) -> serde_json::Value {
    let changed = current.map(|current| {
        let (a, b) = (&previous.cursor, &current.cursor);
        [
            ("scene", a.scene_id != b.scene_id), ("revision", a.revision != b.revision),
            ("terrain_revision", a.terrain_revision != b.terrain_revision),
            ("ground_snapshot", a.ground_snapshot != b.ground_snapshot), ("tool", a.tool != b.tool),
            ("camera", a.camera != b.camera), ("size", a.size != b.size),
            ("camera_entity", a.camera_entity != b.camera_entity), ("projection", a.projection != b.projection),
            ("view", a.view != b.view), ("orthographic", a.orthographic != b.orthographic),
            ("extent", a.extent != b.extent), ("visibility", a.visibility != b.visibility),
            ("asset", a.asset != b.asset || a.asset_target != b.asset_target), ("portal", a.portal != b.portal),
            ("relocation", a.relocation != b.relocation), ("translation_snap", a.translation_snap != b.translation_snap),
            ("input_authority", previous.input_authority != current.input_authority),
            ("input_epoch", previous.input_epoch != current.input_epoch), ("terrain", previous.terrain != current.terrain),
            ("target_surface", previous.surface != current.surface),
            ("surface_revision", previous.surface_revision != current.surface_revision),
            ("layer", previous.layer != current.layer), ("preset", previous.preset != current.preset),
        ].into_iter().filter_map(|(name, changed)| changed.then_some(name)).collect::<Vec<_>>()
    });
    serde_json::json!({"changed":changed,"current_captured":current.is_some(),
        "canvas_writable":terrain_documents::canvas_writable(session, editor),
        "input_authority":cursor.and_then(TerrainGroundCursor::input_authority),
        "ray_epoch":cursor.and_then(|cursor|cursor.ray.map(|ray|ray.epoch)),
        "identity_revision":cursor.and_then(|cursor|cursor.identity.as_ref().map(|identity|identity.revision)),
        "previous_snapshot":previous.cursor.ground_snapshot.as_ref().map(CursorGroundSnapshot::validation_identity),
        "current_snapshot":cursor.and_then(|cursor|cursor.identity.as_ref())
            .and_then(|identity|identity.ground_snapshot.as_ref()).map(CursorGroundSnapshot::validation_identity),
        "revision":session.revision,"terrain_revision":session.terrain_revision,
        "surface_revision":session.surface_revision,"saving":session.saving,"status":editor.status})
}
