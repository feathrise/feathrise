//! 图片与模型中央预览的固定宿主、标签和入口；不增加第二套画布或离屏相机。

use bevy::prelude::*;

#[derive(Component, Clone, Default)]
pub(crate) struct AssetPreviewCenterHost;
#[derive(Component, Clone, Default)]
pub(super) struct AssetPreviewTabRoot;
#[derive(Component, Clone, Default)]
pub(super) struct AssetPreviewSidebarLaunch;
#[derive(Component, Clone, Default)]
pub(super) struct AssetPreviewExpandButton;
#[derive(Component, Clone, Default)]
pub(super) struct AssetPreviewExpandLabel;
#[derive(Component, Clone, Default)]
pub(super) struct AssetPreviewDocumentLaunch;

/// 中央标签动作独立于缩放和子模型选择，不能被导航系统当作画布手势。
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum AssetPreviewWorkspaceAction {
    #[default]
    ToggleExpand,
    OpenSelected,
    RestoreSidebar,
    ActivateTab(u64), // 明确标签ID，返回既有文件时恢复该文件的观察状态。
    CloseTab(u64),
}
