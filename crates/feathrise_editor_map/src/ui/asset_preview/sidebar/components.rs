//! 资产页固定文本、文件路径行及带选择身份的显式复制按钮。

use super::types::SidebarField;
use bevy::prelude::*;

#[derive(Component, Clone, Copy, Default)]
pub(super) struct AssetSidebarText(pub(super) SidebarField);

#[derive(Component, Clone, Copy, Default)]
pub(super) struct AssetSidebarPathRow;

/// 文件切换后旧按下不能复制新文件；反馈只属于按钮绑定的同一路径。
#[derive(Component, Clone, Default)]
pub(super) struct AssetSidebarCopyPath {
    pub(super) path: Option<String>,
    pub(super) feedback: String,
}
