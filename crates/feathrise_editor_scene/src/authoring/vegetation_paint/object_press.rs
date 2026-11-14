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
    surface: ([u8; 32], Mat4),
    vegetation_revision: u64,
    layer: (usize, String),
    erase: bool,
}

impl ObjectVegetationPress {
    fn matches(&self, current: &Self) -> bool {
        let mut expected = self.clone();
        // 作者页到达和兄弟对象的采用可以重建全场查询快照。仅继承仍属于同一目标内容和姿态的按压，
        // 落点另由光标核对最新完整快照、射线和可见面，旧结果不能写入。
        expected.cursor.ground_snapshot = current.cursor.ground_snapshot.clone();
        expected == *current
    }
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
        surface: snapshot.object_paint_identity(&active.definition.id)?,
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
    if pending.as_ref().is_some_and(|previous| current.as_ref().is_none_or(|current| !previous.matches(current))) {
        #[cfg(feature = "validation")]
        if let Some(previous) = pending.as_ref() {
            // 每次按压至多记录一次真正取消；只读固定大小身份，不能为诊断恢复旧按压权限。
            eprintln!("二维植被按压取消：{}", cancellation(previous, current.as_ref(), available,
                editor, session, cursor, mouse.pressed(MouseButton::Left)));
        }
        *pending = None; // 松手、失焦、换层或画布换代永久取消；恢复旧状态不能继承之前的按压。
    }
    if mouse.just_pressed(MouseButton::Left) {
        *pending = current;
        if pending.is_some() && editor.brush_cursor_world.is_none() {
            editor.status = "正在等待所选网格的真实表面；按住继续刷草，松开取消起笔".into();
        }
    }
}

#[cfg(feature = "validation")]
fn cancellation(previous: &ObjectVegetationPress, current: Option<&ObjectVegetationPress>, available: bool,
    editor: &SceneEditorState, session: &EditorAuthoringSession, cursor: Option<&TerrainGroundCursor>,
    held: bool) -> serde_json::Value {
    let changed = current.map(|current| {
        let (a, b) = (&previous.cursor, &current.cursor);
        // 字段名而非整份快照足以定位首个失效门，避免复制场景、像素或后台查询数据。
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
            ("vegetation_revision", previous.vegetation_revision != current.vegetation_revision),
            ("layer", previous.layer != current.layer), ("erase", previous.erase != current.erase),
        ].into_iter().filter_map(|(name, changed)| changed.then_some(name)).collect::<Vec<_>>()
    });
    let identity = cursor.and_then(|cursor|cursor.identity.as_ref());
    serde_json::json!({"changed":changed,"available":available,"held":held,"current_captured":current.is_some(),
        "canvas_writable":terrain_documents::canvas_writable(session, editor),
        "input_authority":cursor.and_then(TerrainGroundCursor::input_authority),
        "ray_present":cursor.is_some_and(|cursor|cursor.ray.is_some()),
        "identity_revision":identity.map(|identity|identity.revision),
        "previous_projection":previous.cursor.projection.to_cols_array(),
        "current_projection":identity.map(|identity|identity.projection.to_cols_array()),
        "previous_snapshot":previous.cursor.ground_snapshot.as_ref().map(CursorGroundSnapshot::validation_identity),
        "current_snapshot":identity.and_then(|identity|identity.ground_snapshot.as_ref()).map(CursorGroundSnapshot::validation_identity),
        "revision":session.revision,"vegetation_revision":session.vegetation_revision,
        "saving":session.saving,"editor_open":editor.open,"status":editor.status})
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
    expected.matches(&current)
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
