//! 边界共享光标的固定身份；悬停不扫描所有墙或复制点集。
use crate::authoring::{BoundaryEditState, EditorAuthoringSession};
use crate::viewport::vegetation_ground::GroundCursorTarget;

pub(crate) fn clipmap_cursor_target(
    session: &EditorAuthoringSession,
    edit: Option<&BoundaryEditState>,
    insert: bool,
) -> Option<GroundCursorTarget> {
    let map = &session.draft.as_ref()?.scene_id;
    let (selected_scene, path_id) = session.selected_boundary_path.as_ref()?;
    if selected_scene != map {
        return None; // 切换场景后旧选择不能武装作者射线。
    }
    if let Some(edit) = edit.filter(|edit| edit.dragging()) {
        if edit.scene_id.as_ref() != Some(map) || edit.before.as_ref()?.id != *path_id {
            return None; // 其他墙的拖点不能回退为当前墙的新建点击。
        }
        Some(GroundCursorTarget::BoundaryDrag {
            path_id: path_id.clone(),
            point_index: edit.point_index?,
        })
    } else {
        Some(GroundCursorTarget::BoundaryPoint {
            path_id: path_id.clone(),
            insert,
        })
    }
}
