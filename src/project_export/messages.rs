//! 工程导出低频意图；按钮只提交请求，构建与文件事务统一在后台执行。

use bevy::prelude::*;
use std::path::PathBuf;

#[derive(Message, Clone, Copy)]
pub(crate) struct OpenExportDialog;

#[derive(Message, Clone)]
pub(crate) struct RequestExport {
    pub(crate) destination: PathBuf,
}

#[derive(Message, Clone, Copy)]
pub(crate) struct CancelExport;
