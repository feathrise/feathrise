//! 完整预览、磁盘/草稿快照和批次采用身份；正文共享Arc，UI只读取有界片段。

use super::super::super::{history::Selection, navigation::ReplacedContent, types::DiskIdentity};
use super::super::types::{DraftStamp, SearchResult};
use std::{
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
};

#[derive(Clone)]
pub(super) struct DraftInput {
    pub stamp: DraftStamp,
    pub disk: DiskIdentity,
    pub source: Arc<str>,
    pub pages: Vec<Arc<str>>,
    pub selection: Selection,
}

pub(super) struct PreviewFile {
    pub index: usize, // 搜索结果中的固定文件序号，不能用可复用UI行槽当身份。
    pub draft: Option<DraftStamp>,
    pub disk: DiskIdentity,
    pub source: Arc<str>,
    pub pages: Vec<Arc<str>>,
    pub selection: Selection,
    pub content: Option<ReplacedContent>,
    pub after: Vec<String>,
}

pub(super) struct ReplacePlan {
    pub ticket: u64,
    pub search_ticket: u64,
    pub root: PathBuf,
    pub search: Arc<SearchResult>,
    pub replacement: String,
    pub files: Vec<PreviewFile>,
}

#[derive(Clone, Copy, Debug)]
pub(super) enum Target {
    Checked,
    File(usize),
    Hit(usize, usize),
}

#[derive(Clone)]
pub(super) enum Work {
    Preview {
        search: Arc<SearchResult>,
        query: String,
        replacement: String,
        case_sensitive: bool,
        whole_word: bool,
        checked: Vec<usize>,
        drafts: Vec<DraftInput>,
    },
    Apply {
        plan: Arc<ReplacePlan>,
        target: Target,
    },
}

#[derive(Clone)]
pub(super) struct Request {
    pub ticket: u64,
    pub cancel: Arc<AtomicBool>,
    pub work: Work,
}

pub(super) struct AppliedFile {
    pub index: usize,
    pub content: ReplacedContent,
}

pub(super) enum Outcome {
    Preview(Result<Arc<ReplacePlan>, String>),
    Apply {
        plan: Arc<ReplacePlan>,
        content: Result<Vec<AppliedFile>, String>,
    },
}

pub(super) struct Reply {
    pub ticket: u64,
    pub outcome: Outcome,
}

pub(super) struct BatchHistory {
    pub stamps: Vec<DraftStamp>,
    pub undone: bool,
}
