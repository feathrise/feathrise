//! 空气墙面板标记；普通动作与对象行接入共享按钮释放协议，名称框保留官方文本焦点。
use feathrise_editor_ui::InteractionButton;
use bevy::prelude::*; // ECS组件。
#[derive(Component, Clone, Default)]
pub(crate) struct WallPanelSlot; // 固定挂载槽。
#[derive(Component)]
#[require(InteractionButton)] // 固定行也必须注册共享捕获，独立选择底色继续由对象面板回显。
pub(super) struct WallRow(pub usize); // 页内固定行号。
#[derive(Component)]
pub(super) struct WallLabel(pub Entity); // 文本关联交互行。
#[derive(Component)]
pub(super) struct WallTitle; // 页码和对象总数。
#[derive(Component)]
pub(super) struct WallName; // 官方名称输入框。
/// 固定控件只切显隐；不销毁名称实体，也不另建对象动作或事务。
#[derive(Component, Clone, Copy)]
pub(super) enum WallControls {
    Pagination,
    Selection,
}
#[derive(Component, Clone, Copy)]
#[require(InteractionButton)] // 所有普通操作统一在原按钮内松开确认，不把名称输入框改成按钮。
pub(super) enum Action {
    // 操作只针对当前稳定对象。
    CreateInternal,
    CreateOuter,
    Copy,
    Delete,
    Enabled,
    Visible,
    Rename,
    Previous,
    Next,
}
