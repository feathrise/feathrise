//! 唯一文档导航会话及检索后台槽；切文件不会生成排队任务或第二份全文输入。

use super::{super::types::Document, types::*};
use bevy::{prelude::*, tasks::Task};
use std::{ops::Range, sync::Arc};

#[derive(Resource, Default)]
pub(super) struct DocumentNavigation {
    pub mode: NavigationMode,
    pub document: Option<super::super::types::SourceKey>,
    pub query: String,
    pub replacement: String,
    pub options: SearchOptions,
    pub in_selection: bool,
    pub scope: Option<SelectionScope>,
    pub line: String,
    pub ticket: u64,
    pub index: Option<Arc<TextIndex>>,
    pub task: Option<Task<NavigationWork>>,
    pub operation: u64,
    pub replace: Option<ReplacementRequest>,
    pub ready: Option<ReplacementResult>,
    pub selected: Option<usize>,
    pub pending: Option<NavigationRequest>,
    pub reveal: Option<RevealTarget>,
    pub hint: String,
}

impl DocumentNavigation {
    pub(super) fn scope_for(&self, doc: &Document) -> Option<Range<usize>> {
        self.in_selection.then(|| {
            self.scope
                .as_ref()
                .filter(|scope| {
                    scope.document.path == doc.path
                        && scope.document.generation == doc.generation
                        && scope.revision == doc.revision
                })
                .map_or(0..0, |scope| scope.range.clone())
        }) // 失效选区只能产生零匹配，绝不能静默扩大为整篇替换。
    }

    pub(super) fn invalidate(&mut self) {
        self.ticket = self.ticket.wrapping_add(1);
        self.index = None;
        self.selected = None;
        self.pending = None;
        self.reveal = None;
        self.cancel_replace();
    }

    pub(super) fn cancel_replace(&mut self) {
        if self.replace.is_some() || self.ready.is_some() {
            self.hint = "替换已取消；草稿保持".into();
        }
        self.replace = None;
        self.ready = None; // task仍持有到真正结束，取消不能腾出另一个worker槽。
    }
}

#[derive(Resource, Default)]
pub(super) struct NavigationPaint {
    pub key: Option<PaintKey>,
    pub rows: Vec<RowSlot>,
    pub atlas: Handle<Image>,
    pub overview: Handle<Image>,
    pub overview_cursor: usize,
    pub overview_pixels: Vec<u8>,
    pub overview_ready: bool,
}
