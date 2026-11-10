//! 菜单与搜索结果的显式外部打开意图；打开磁盘文件不提交内置草稿。

use super::types::ExternalFileAction;
use bevy::prelude::*;

#[derive(Message, Clone)]
pub(crate) struct OpenExternalFile {
    pub path: String,
    pub action: ExternalFileAction,
    pub has_draft: bool,
}

/// 工程文件入口冻结显式根；只交给既有唯一IO槽，不把源码路径解释为资产路径。
#[derive(Message, Clone)]
pub(crate) struct OpenExternalProjectFile {
    pub root: std::path::PathBuf,
    pub request: OpenExternalFile,
}
