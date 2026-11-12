//! 全文索引和批量替换共用的草稿版本及区间身份。

use crate::{history::types::Selection, types::SourceKey};
use std::{ops::Range, sync::Arc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchOptions {
    pub case_sensitive: bool,
    pub whole_word: bool,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            case_sensitive: true,
            whole_word: false,
        } // 保持已有字面查找的默认行为。
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexKey {
    pub document: SourceKey,
    pub revision: u64,
    pub query: String,
    pub ticket: u64,
    pub options: SearchOptions,
    pub scope: Option<Range<usize>>,
}

pub struct TextIndex {
    pub key: IndexKey,
    pub lines: Vec<usize>,
    pub matches: Vec<Range<usize>>,
    pub error: Option<String>,
}

pub struct ReplacedContent {
    pub source: Arc<str>,
    pub pages: Vec<Arc<str>>,
    pub selection: Selection,
    pub scope: Option<Range<usize>>,
    pub count: usize,
}
