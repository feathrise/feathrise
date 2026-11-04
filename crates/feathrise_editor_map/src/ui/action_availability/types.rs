//! 有界状态快照及统一阻塞原因；不代替作者事务的最终身份与范围校验。

use crate::types::EditorAction;
use feathrise_editor_core::EditorShortcutAction;

#[derive(Default)]
pub(in crate::ui) struct EditorActionAvailability {
    pub(in crate::ui) blocked: bool,
    pub(in crate::ui) mesh_editing: bool,
    pub(in crate::ui) other_workspace: bool,
    pub(in crate::ui) current_map: bool,
    pub(in crate::ui) viewport_ready: bool,
    pub(in crate::ui) saving: bool,
    pub(in crate::ui) undo_empty: bool,
    pub(in crate::ui) redo_empty: bool,
    pub(in crate::ui) terrain_pending: bool,
    pub(in crate::ui) deletion_busy: bool,
    pub(in crate::ui) focus_available: bool,
}

impl EditorActionAvailability {
    /// 本轮仅收口十个地图命令；新建、关闭、布局和设置保留原有独立资格。
    pub(in crate::ui) fn supports(action: EditorAction) -> bool {
        matches!(
            action,
            EditorAction::Save
                | EditorAction::Undo
                | EditorAction::Redo
                | EditorAction::ViewPerspective
                | EditorAction::ViewTop
                | EditorAction::ViewFront
                | EditorAction::ViewSide
                | EditorAction::ToggleOrthographic
                | EditorAction::FocusSelected
                | EditorAction::FrameMap
        )
    }

    pub(in crate::ui) fn shortcut_action(action: EditorShortcutAction) -> Option<EditorAction> {
        match action {
            EditorShortcutAction::Save => Some(EditorAction::Save),
            EditorShortcutAction::Undo => Some(EditorAction::Undo),
            EditorShortcutAction::Redo => Some(EditorAction::Redo),
            EditorShortcutAction::FocusSelected => Some(EditorAction::FocusSelected),
            EditorShortcutAction::FrameMap => Some(EditorAction::FrameMap),
            _ => None,
        }
    }

    pub(in crate::ui) fn reason(&self, action: EditorAction) -> Option<&'static str> {
        if !Self::supports(action) {
            return None;
        }
        if self.blocked {
            return Some("请先结束当前弹窗或编辑操作");
        }
        if self.mesh_editing && !matches!(action, EditorAction::Save | EditorAction::Undo | EditorAction::Redo
            | EditorAction::ViewPerspective | EditorAction::ViewTop | EditorAction::ViewFront | EditorAction::ViewSide | EditorAction::ToggleOrthographic) {
            return Some("请先按 Tab 返回对象模式");
        }
        if self.other_workspace {
            return Some("请先切换到地图页");
        }
        if !self.current_map {
            return Some("尚未进入就绪地图");
        }
        if self.deletion_busy {
            return Some("地图回收事务尚未完成");
        }
        match action {
            EditorAction::Save | EditorAction::Undo | EditorAction::Redo if self.saving => {
                Some("当前地图正在保存")
            }
            EditorAction::Undo if self.undo_empty => Some("当前地图没有可撤销操作"),
            EditorAction::Redo if self.redo_empty => Some("当前地图没有可重做操作"),
            EditorAction::Save if self.terrain_pending => Some("地形高度、法线与碰撞仍在后台生成"),
            EditorAction::ViewPerspective
            | EditorAction::ViewTop
            | EditorAction::ViewFront
            | EditorAction::ViewSide
            | EditorAction::ToggleOrthographic
            | EditorAction::FocusSelected
            | EditorAction::FrameMap
                if !self.viewport_ready =>
            {
                Some("地图视口尚未就绪")
            }
            EditorAction::FocusSelected if !self.focus_available => {
                Some("请先选择可聚焦的资产、水体或传送点")
            }
            _ => None,
        }
    }
}
