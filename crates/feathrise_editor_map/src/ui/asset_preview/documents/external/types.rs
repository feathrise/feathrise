//! 外部修订与有界核对快照；完整内容仅随唯一文档 IO 回执返回。

use super::super::types::{DiskIdentity, SourceContents, SourceKey};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in super::super) enum ExternalIdentity {
    Changed(DiskIdentity),
    Unavailable(String),
}

#[derive(Clone, Debug, Default)]
pub(in super::super) struct ExternalDocument {
    pub observed: Option<ExternalIdentity>,
    pub warning: bool, // 保留草稿只收起同一修订提示；不会把磁盘基线改成外部版本。
}

#[derive(Clone)]
pub(super) struct CheckSnapshot {
    pub key: SourceKey,
    pub revision: u64,
    pub disk: DiskIdentity,
}

pub(super) struct CheckedDocument {
    pub snapshot: CheckSnapshot,
    pub result: Result<(DiskIdentity, SourceContents), String>,
}

pub(in super::super) struct CheckReply {
    pub(super) root: PathBuf,
    pub(super) documents: Vec<CheckedDocument>,
}
