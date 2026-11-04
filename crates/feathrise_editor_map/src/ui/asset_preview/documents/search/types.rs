//! 搜索快照、文件分组、真实UTF-8位置及只读中央定位契约。

use super::super::types::SourceKey;
use std::{
    ops::Range,
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct DraftStamp {
    pub key: SourceKey,
    pub revision: u64,
}

#[derive(Clone)]
pub(super) struct Draft {
    pub stamp: DraftStamp,
    pub text: Arc<str>,
    pub dirty: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ContentIdentity {
    Draft(DraftStamp),
    Disk([u8; 32]),
    ProjectSource([u8; 32]), // 项目源码只读，路径相对项目根；不能作为assets文档加载或替换。
}

#[derive(Clone)]
pub(super) struct SearchRequest {
    pub ticket: u64,
    pub root: PathBuf,
    pub folder: String,
    pub query: String,
    pub references: bool, // JSON路径查询只读；普通引用时query是资产物理路径。
    pub missing_assets: bool, // 缺失检查不需要query，仍属于只读路径查询，不能进入替换。
    pub filters: Vec<String>,
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub drafts: Vec<Draft>,
    pub cancel: Arc<AtomicBool>,
}

#[derive(Clone, Debug)]
pub(super) struct Hit {
    pub range: Range<usize>, // 与文档唯一原生输入相同的LF、无BOM字节坐标。
    pub line: usize,
    pub column: usize, // Unicode字符列，不能把中文UTF-8字节数显示成列号。
    pub snippet: String,
}

pub(super) struct FileHits {
    pub path: String,
    pub identity: ContentIdentity,
    pub dirty: bool,
    pub hits: Vec<Hit>,
}

#[derive(Clone, Copy)]
pub(super) enum ResultRow {
    File(usize),
    Hit(usize, usize),
}

#[derive(Default)]
pub(super) struct SearchResult {
    pub ticket: u64,
    pub references: bool, // 查询只读；替换入口还须核验此标记。
    pub missing_assets: bool,
    pub root: PathBuf,
    pub folder: String,
    pub drafts: Vec<DraftStamp>,
    pub files: Vec<FileHits>,
    pub rows: Vec<ResultRow>,
    pub hits: usize,
    pub scanned: usize,
    pub checked_paths: usize, // 缺失检查已经核对的非空路径；零问题不能冒充未执行或未完成。
    pub skipped: usize,
    pub limited: bool,
    pub cancelled: bool,
    pub error: String,
}

pub(super) struct PendingJump {
    pub path: String,
    pub identity: ContentIdentity,
    pub range: Range<usize>,
}

pub(super) struct JumpReveal {
    pub source: bevy::prelude::Entity,
    pub document: SourceKey,
    pub revision: u64,
    pub range: Range<usize>,
}
