//! 历史快照与输入事务的数据；内容身份和派生任务修订分开，避免撤销采用迟到结果。

use std::{collections::VecDeque, sync::Arc, time::Instant};

/// 文档工具只共享输入顺序，不公开历史按钮或系统参数类型。
#[derive(bevy::prelude::SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub(in super::super) enum HistorySystemSet {
    Input,
}

pub(in super::super) use feathrise_editor_documents_model::history::types::Selection;

#[derive(Clone, Copy)]
pub(in super::super) struct EditContext {
    pub before: Selection,
    pub after: Selection,
    pub typing: bool,
    pub at: Instant,
}

impl Default for EditContext {
    fn default() -> Self {
        Self {
            before: Selection::default(),
            after: Selection::default(),
            typing: false,
            at: Instant::now(),
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct Snapshot {
    pub source: Arc<str>,
    pub pages: Vec<Arc<str>>,
    pub content_revision: u64,
    pub selection: Selection,
}

impl Snapshot {
    pub(super) fn bytes(&self) -> usize {
        self.source.len() + self.pages.iter().map(|page| page.len()).sum::<usize>()
    }
}

#[derive(Clone, Debug)]
pub(super) struct Entry {
    pub before: Snapshot,
    pub after: Snapshot,
}

impl Entry {
    pub(super) fn bytes(&self) -> usize {
        self.before.bytes() + self.after.bytes()
    }
}

#[derive(Clone, Debug)]
pub(super) struct TypingGroup {
    pub at: Instant,
    pub selection: Selection,
}

#[derive(Clone, Debug, Default)]
pub(in super::super) struct DocumentHistory {
    pub(super) undo: VecDeque<Entry>,
    pub(super) redo: Vec<Entry>,
    pub(super) bytes: usize, // 相邻Arc虽共享，仍逐条保守计费，不为了精确去重扫描全文。
    pub(super) group: Option<TypingGroup>,
    pub(super) content_revision: Option<u64>, // None时沿用文档初始/既有修订。
}
