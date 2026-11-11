//! 唯一外部启动任务与一个合并后的待办，快速连点不会无限创建进程任务。

use super::messages::OpenExternalFile;
use super::types::{DetectedEditors, ExternalTaskReply};
use bevy::{prelude::*, tasks::Task};
use std::path::PathBuf;

#[derive(Resource)]
pub(super) struct ExternalFileState {
    pub root: PathBuf,
    pub pending: Option<OpenExternalFile>,
    pub pending_project: Option<super::messages::OpenExternalProjectFile>, // 与pending互斥，共用同一后台启动槽。
    pub task: Option<Task<ExternalTaskReply>>,
    pub scan_requested: bool,
    pub was_open: bool,
}

/// 未完成检测时默认无IDE选项；刷新期间保留上一完整缓存，采用后只显示可用入口。
#[derive(Resource, Clone, Default, PartialEq, Eq)]
pub(crate) struct ExternalEditors {
    pub(super) detected: DetectedEditors,
}

impl Default for ExternalFileState {
    fn default() -> Self {
        Self {
            root: feathrise_editor_core::workspace_asset_root(), // IDE定位与文档作者会话共用当前工程来源。
            pending: None,
            pending_project: None,
            task: None,
            scan_requested: true,
            was_open: false,
        }
    }
}
