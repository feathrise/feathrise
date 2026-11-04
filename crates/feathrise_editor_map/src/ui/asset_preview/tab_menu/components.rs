//! 文件标签菜单的固定遮罩、浮层和关闭按钮，不进入普通作者动作分派。
use bevy::prelude::*;

#[derive(Component)]
pub(super) struct TabMenuBackdrop;
#[derive(Component)]
pub(super) struct TabMenuPopup;
#[derive(Component)]
pub(super) struct CloseAllTabsButton;
#[derive(Component)]
pub(super) struct CloseUnmodifiedTabsButton;

#[derive(Component)]
pub(super) struct SaveAllDraftsButton;

#[derive(Component)]
pub(super) struct ExternalFileButton(pub crate::ui::external_files::ExternalFileAction);

/// 文件标签复制沿用资产菜单的同一协议，空白标签组不提供文件动作。
#[derive(Component)]
pub(super) struct CopyFilePathButton(pub crate::ui::asset_context_menu::AssetPathCopyKind);
