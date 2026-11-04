//! 插件管理浮窗的固定控件与冻结身份；目录变化不能把旧释放点击转交给另一插件。

use bevy::prelude::*;
use super::types::PluginScope;

#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub(super) enum ManagerAction {
    Close,
    Refresh,
    Install,
    OpenDirectory,
    SetScope(PluginScope),
    Select(String),
    SetEnabled { id: String, enabled: bool },
    Update(String),
    RequestRemove(String),
    ConfirmRemove,
    CancelRemove,
    CancelPending(String),
}

/// 所有有目标的动作冻结本次列表修订；后台刷新或换绑会撤销尚未释放的旧点击。
#[derive(Component, Default)]
pub(super) struct ManagerBinding(pub u64);

#[derive(Component)]
pub(super) struct ManagerPanel;
#[derive(Component)]
pub(super) struct ManagerHeader;
#[derive(Component)]
pub(super) struct ManagerBody;
#[derive(Component)]
pub(super) struct ManagerFooter;
#[derive(Component)]
pub(super) struct ManagerOverlay;
#[derive(Component)]
pub(super) struct ManagerPane;
#[derive(Component)]
pub(super) struct ManagerContent;
#[derive(Component)]
pub(super) struct ManagerDetailsPane;
#[derive(Component)]
pub(super) struct ManagerScrollTrack;
#[derive(Component)]
pub(super) struct ManagerScrollThumb;

#[derive(Component, Clone, Copy)]
pub(super) enum ManagerLayout {
    Overlay,
    Columns,
    ListRegion,
    Empty,
    Content,
    Confirmation,
}

#[derive(Component, Clone, Copy)]
pub(super) enum ManagerControl {
    Toggle,
    Update,
    Remove,
    CancelPending,
    Confirm,
    CancelConfirm,
    Install,
    Refresh,
    OpenDirectory,
    Scope(PluginScope),
}

#[derive(Component)]
pub(super) struct ManagerRow {
    pub slot: usize,
    pub index: Option<usize>,
    pub id: Option<String>,
    pub revision: u64,
}

#[derive(Clone, Copy)]
pub(super) enum DetailKind {
    Name,
    Identity,
    Path,
    State,
    Error,
    Pending,
    Runtime,
    Source,
}

#[derive(Component, Clone, Copy)]
pub(super) enum ManagerText {
    Count,
    Status,
    Row { slot: usize, line: u8 },
    Detail(DetailKind),
    Toggle,
    Confirmation,
    ScopeInfo,
    Directory,
}
