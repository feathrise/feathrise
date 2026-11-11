//! 底栏只读阶段、固定文本角色和详情动作，不定义新的作者命令。

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum StatusPhase {
    #[default]
    Ready,
    Working,
    Success,
    Warning,
    Error,
}

impl StatusPhase {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Ready => "就绪",
            Self::Working => "处理中",
            Self::Success => "已完成",
            Self::Warning => "需处理",
            Self::Error => "失败",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum StatusTextRole {
    #[default]
    Phase,
    Summary,
    Task,
    Context,
    Detail,
    Title,
    CopyResult,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum StatusAction {
    #[default]
    Toggle,
    Copy,
    Dismiss,
    Close,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RuntimePhase {
    Working,
    Ready,
    Disabled,
    Failed,
}

/// 固定五项运行时证据只借用诊断；旧场景身份不会进入当前底栏。
#[derive(Clone, Copy)]
pub(super) struct RuntimeEntry<'a> {
    pub(super) label: &'static str,
    pub(super) phase: Option<RuntimePhase>,
    pub(super) detail: &'a str,
}
