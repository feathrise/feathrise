//! 关闭确认层的按钮和展示节点；按钮继续采用原松开执行协议。

use bevy::prelude::*;

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CloseAction {
    /// 仅在工程干净时确认当前关闭目标，不包含保存或放弃任何草稿的授权。
    Close,
    Save,
    Discard,
    Cancel,
}

#[derive(Component)]
pub(super) struct CloseOverlay;
#[derive(Component)]
pub(super) struct ClosePanel;
#[derive(Component)]
pub(super) struct CloseActions;
#[derive(Component)]
pub(super) struct CloseHeader;
#[derive(Component)]
pub(super) struct CloseBody;
#[derive(Component)]
pub(super) struct CloseFooter;
#[derive(Component)]
pub(super) struct CloseHeaderClose;
#[derive(Component)]
pub(super) struct CloseStatus;
#[derive(Component)]
pub(super) struct CloseTitle;
#[derive(Component)]
pub(super) struct CloseActionLabel(pub CloseAction);
#[derive(Component)]
pub(super) struct CloseDocuments;
#[derive(Component)]
pub(super) struct CloseScroll;
