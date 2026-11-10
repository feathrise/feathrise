//! 工程窗口之间的控制回执；不含作者文档、ECS实体或GPU句柄。
use crate::workspace_ui::{WorkspaceCommand, WorkspaceField, WorkspaceModel};
use crate::native_project_close::CloseTarget;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) enum AuthorEvent {
    Ready { hwnd: usize },
    /// Unix由token和实际共享帧确认呈现，不能用伪HWND混入Win32身份检查。
    ReadySurface { session: String },
    #[cfg(windows)]
    FramePacing(crate::editor_frame_pacing::FramePacingRequest),
    Command(WorkspaceCommand),
    CloseCancelled,
    /// 旧单位回执继续解码为工程关闭；新版显式带目标，避免迟到取消撤回另一种关闭意图。
    CloseCancelledFor { target: CloseTarget },
    Failed(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) enum HostEvent {
    Model(WorkspaceModel),
    FillPath {
        field: WorkspaceField,
        value: String,
    },
    RequestClose,
    /// 主窗口X/退出菜单必须在作者端显示退出编辑器，不能沿用关闭工程的确认授权。
    RequestExitEditor,
    CancelClose,
    AbortCandidate,
    Activate,
}

impl HostEvent {
    pub(super) fn close_request(target: CloseTarget) -> Self {
        match target {
            CloseTarget::Project => Self::RequestClose,
            CloseTarget::Editor => Self::RequestExitEditor,
        }
    }

    pub(super) fn close_target(&self) -> Option<CloseTarget> {
        match self {
            Self::RequestClose => Some(CloseTarget::Project),
            Self::RequestExitEditor => Some(CloseTarget::Editor),
            _ => None,
        }
    }
}

impl AuthorEvent {
    pub(super) fn cancelled_target(&self) -> Option<CloseTarget> {
        match self {
            Self::CloseCancelled => Some(CloseTarget::Project),
            Self::CloseCancelledFor { target } => Some(*target),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_and_editor_requests_round_trip_without_changing_legacy_units() {
        for (target, expected) in [(CloseTarget::Project, "\"RequestClose\""), (CloseTarget::Editor, "\"RequestExitEditor\"")] {
            let json = serde_json::to_string(&HostEvent::close_request(target)).unwrap();
            assert_eq!(json, expected);
            let decoded: HostEvent = serde_json::from_str(&json).unwrap();
            assert_eq!(decoded.close_target(), Some(target));
        }
        assert_eq!(serde_json::to_string(&AuthorEvent::CloseCancelled).unwrap(), "\"CloseCancelled\"");
        let legacy: AuthorEvent = serde_json::from_str("\"CloseCancelled\"").unwrap();
        assert_eq!(legacy.cancelled_target(), Some(CloseTarget::Project));
    }

    #[test]
    fn cancellation_round_trip_keeps_the_target_which_was_actually_cancelled() {
        for target in [CloseTarget::Project, CloseTarget::Editor] {
            let event = AuthorEvent::CloseCancelledFor { target };
            let decoded: AuthorEvent = serde_json::from_str(&serde_json::to_string(&event).unwrap()).unwrap();
            assert_eq!(decoded.cancelled_target(), Some(target));
        }
    }
}
