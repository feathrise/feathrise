//! 单窗口工程页面的纯数据合同；只传工程操作，不承载作者World或文档历史。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct WorkspaceProject {
    pub name: String,
    pub root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RecentEntry {
    pub name: String,
    pub path: PathBuf,
    pub last_opened: String,
    #[serde(default)]
    pub pinned: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Page {
    #[default]
    Start,
    New,
    Open,
    Editor,
    Empty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum WorkspaceField {
    NewParent,
    OpenPath,
    DefaultDirectory,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ProbeButton {
    pub action: String,
    pub rect: [f32; 4],
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ProbeField {
    pub field: String,
    pub value: String,
    pub rect: [f32; 4],
}
