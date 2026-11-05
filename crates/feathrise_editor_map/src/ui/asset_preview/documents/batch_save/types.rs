//! 一次显式保存的不可变文件快照、轻量回执及报告；路径代次和内容保存点分别核验。

use super::super::types::{DiskIdentity, SourceKey};
use std::{path::PathBuf, sync::Arc};

pub(in super::super) enum SaveScope {
    All,
    Batch(Vec<SourceKey>), // 最近实际替换的文件，不能用当前搜索勾选替代真实应用范围。
}

pub(super) struct SaveInput {
    pub index: usize,
    pub key: SourceKey,
    pub revision: u64, // 内容身份；不是单调增长的派生修订。
    pub disk: DiskIdentity,
    pub pages: Vec<Arc<str>>,
    pub source: Arc<str>,
}

pub(super) struct FileReceipt {
    pub index: usize,
    pub key: SourceKey,
    pub revision: u64,
    pub result: Result<DiskIdentity, String>,
}

pub(in super::super) struct SaveReply {
    pub(super) ticket: u64,
    pub(super) root: PathBuf,
    pub(super) files: Vec<FileReceipt>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum FileOutcome {
    Pending,
    Saved,
    Failed,
    Unmodified,
}

pub(super) struct FileReport {
    pub path: String, // 完整文档身份；源码和资产的显示路径相同也必须是两份独立保存项。
    pub outcome: FileOutcome,
    pub detail: String,
}
