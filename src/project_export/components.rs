//! 工程导出弹窗的固定控件标记；文件事务和构建状态由导出后台拥有。

use bevy::prelude::*;

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ExportAction {
    Close,
    Browse,
    Start,
    Cancel,
    OpenDirectory,
}

#[derive(Component)]
pub(super) struct ExportOverlay;
#[derive(Component)]
pub(super) struct ExportDestination;
#[derive(Component)]
pub(super) struct ExportBody;
#[derive(Component)]
pub(super) struct ExportStatus;
#[derive(Component)]
pub(super) struct ExportProfile;
#[derive(Component)]
pub(super) struct ExportActionLabel(pub ExportAction);
