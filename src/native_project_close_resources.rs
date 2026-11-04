//! 关闭选择只保留退出授权；放弃不清原脏位、不改草稿，也不伪造保存成功。

use super::components::CloseAction;
use bevy::prelude::*;

/// 同一作者进程退出可能只释放工程，也可能结束整个编辑器；目标必须随宿主请求显式传递。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum CloseTarget {
    Project,
    #[default]
    Editor,
}

impl CloseTarget {
    pub(crate) fn operation(self) -> &'static str {
        match self {
            Self::Project => "关闭工程",
            Self::Editor => "退出编辑器",
        }
    }

    pub(super) fn title(self) -> &'static str {
        match self {
            Self::Project => "关闭当前工程",
            Self::Editor => "退出编辑器",
        }
    }

    pub(super) fn action_label(self, action: CloseAction) -> &'static str {
        match (self, action) {
            (_, CloseAction::Cancel) => "取消 (Esc)",
            (Self::Project, CloseAction::Close) => "关闭工程",
            (Self::Project, CloseAction::Save) => "保存并关闭",
            (Self::Project, CloseAction::Discard) => "不保存并关闭",
            (Self::Editor, CloseAction::Close) => "退出编辑器",
            (Self::Editor, CloseAction::Save) => "保存并退出",
            (Self::Editor, CloseAction::Discard) => "不保存并退出",
        }
    }

    pub(super) fn confirmation(self, documents: usize) -> String {
        if documents == 0 {
            format!("确定{}？", self.title())
        } else {
            let verb = match self { Self::Project => "关闭", Self::Editor => "退出" };
            format!("有 {documents} 项未保存修改，是否保存后{verb}？")
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum CloseStage {
    #[default]
    Idle,
    Inspecting,
    Confirm,
    Waiting,
    Blocked,
    Saving,
    Draining,
}

#[derive(Resource, Default)]
pub(crate) struct NativeProjectCloseState {
    pub(super) target: CloseTarget,
    pub(super) stage: CloseStage,
    pub(super) choice: Option<CloseAction>,
    pub(super) intent: Option<CloseAction>,
    pub(super) documents: Vec<String>,
    pub(super) status: String,
    pub(super) failed: Option<String>,
    pub(super) host_blocked: Option<String>,
    pub(super) input_active: bool,
    pub(super) focus_cancel: bool,
    pub(super) startup_save_pending: bool,
}

impl NativeProjectCloseState {
    pub(crate) fn target(&self) -> CloseTarget {
        self.target
    }

    pub(crate) fn set_target(&mut self, target: CloseTarget) {
        if self.target == target {
            return;
        }
        self.target = target;
        // 不能将“仅关闭工程”的确认提升成退出编辑器；保留原写者和错误，重新取得明确选择。
        self.choice = None;
        self.intent = None;
        self.focus_cancel = true;
        if self.active() && !matches!(self.stage, CloseStage::Saving | CloseStage::Blocked) {
            self.stage = CloseStage::Inspecting;
        }
    }

    pub(crate) fn active(&self) -> bool {
        self.stage != CloseStage::Idle
    }

    pub(crate) fn captures_input(&self) -> bool {
        self.input_active || self.active()
    }

    /// 已写盘但未被正式采用的失败仍需重试或明确放弃，不能仅凭没有脏文档显示直接关闭。
    pub(super) fn needs_save_decision(&self) -> bool {
        !self.documents.is_empty() || self.failed.is_some()
    }

    pub(super) fn action_visible(&self, action: CloseAction) -> bool {
        match action {
            CloseAction::Close => !self.needs_save_decision(),
            CloseAction::Save | CloseAction::Discard => self.needs_save_decision(),
            CloseAction::Cancel => true,
        }
    }

    /// 展示和输入共用动作门，状态切换后不能消费刚隐藏按钮的迟到松开事件。
    pub(super) fn action_enabled(&self, action: CloseAction) -> bool {
        action == CloseAction::Cancel
            || self.stage == CloseStage::Confirm && self.action_visible(action)
    }

    pub(crate) fn hold(&mut self, reason: String) {
        self.stage = CloseStage::Blocked;
        self.status.clone_from(&reason);
        self.host_blocked = Some(reason);
        self.focus_cancel = true;
    }

    pub(crate) fn wait_for_startup_save(&mut self) {
        self.startup_save_pending = true;
    }

    pub(crate) fn take_startup_save_wait(&mut self) -> bool {
        std::mem::take(&mut self.startup_save_pending)
    }
}
