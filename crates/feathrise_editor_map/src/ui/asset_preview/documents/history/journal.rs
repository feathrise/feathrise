//! 移动共享快照实现撤销/重做；恢复内容不回退派生修订，保存点按内容身份判断。

use super::super::{io, types::Document};
use super::{constants::*, types::*};
use std::sync::Arc;

impl DocumentHistory {
    pub(in super::super) fn break_group(&mut self) {
        self.group = None;
    }
    pub(in super::super) fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub(in super::super) fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub(super) fn record(&mut self, before: Snapshot, after: Snapshot, context: EditContext) {
        // 新编辑立即截断重做分支；快照只克隆Arc，原生输入与布局从不复制。
        for entry in self.redo.drain(..) {
            self.bytes -= entry.bytes();
        }
        let join = context.typing
            && context.before.anchor == context.before.focus
            && self.group.as_ref().is_some_and(|group| {
                context.at.saturating_duration_since(group.at) <= TYPING_WINDOW
                    && group.selection == context.before
            })
            && self
                .undo
                .back()
                .is_some_and(|entry| entry.after.content_revision == before.content_revision);
        if join {
            let entry = self.undo.back_mut().unwrap();
            self.bytes -= entry.after.bytes();
            self.bytes += after.bytes();
            entry.after = after;
        } else {
            let entry = Entry { before, after };
            self.bytes += entry.bytes();
            self.undo.push_back(entry);
        }
        self.group = context.typing.then_some(TypingGroup {
            at: context.at,
            selection: context.after,
        });
        while self.undo.len() > MAX_ENTRIES || self.bytes > MAX_SNAPSHOT_BYTES {
            let Some(entry) = self.undo.pop_front() else {
                break;
            };
            self.bytes -= entry.bytes();
        }
    }
}

impl Document {
    pub(in super::super) fn content_revision(&self) -> u64 {
        self.history.content_revision.unwrap_or(self.revision)
    }

    fn snapshot(&self, selection: Selection) -> Snapshot {
        Snapshot {
            source: self.source.clone(),
            pages: self.pages.clone(),
            content_revision: self.content_revision(),
            selection,
        }
    }

    #[cfg(test)]
    pub(in super::super) fn edit(&mut self, value: &str) {
        self.edit_with(value, EditContext::default());
    }

    pub(in super::super) fn edit_with(&mut self, value: &str, context: EditContext) {
        if self.source_text() == value {
            return;
        }
        let pages = vec![io::edited_source(
            &self.pages,
            &self.source,
            value,
            self.disk.crlf,
        )];
        self.replace_content(Arc::from(value), pages, context);
    }

    /// 格式化在后台准备完整正文及保存字节；采用只移动Arc并登记同一历史，不再扫描全文。
    pub(in super::super) fn replace_content(&mut self, source: Arc<str>, pages: Vec<Arc<str>>, context: EditContext) {
        let before = self.snapshot(context.before);
        self.pages = pages;
        self.source = source;
        self.revision = self.revision.wrapping_add(1);
        self.history.content_revision = Some(self.revision);
        self.confirm_reload = false;
        self.status = "草稿未保存；切换资产或离开编辑后仍保留，退出程序前请保存".into();
        let after = self.snapshot(context.after);
        self.history.record(before, after, context);
    }

    /// 选区与实际保存字节成对恢复；稳定业务身份继续增长，使旧分词/预览/小地图结果失效。
    pub(in super::super) fn travel(&mut self, redo: bool) -> Option<Selection> {
        self.history.break_group();
        let snapshot = if redo {
            let entry = self.history.redo.pop()?;
            let snapshot = entry.after.clone();
            self.history.undo.push_back(entry);
            snapshot
        } else {
            let entry = self.history.undo.pop_back()?;
            let snapshot = entry.before.clone();
            self.history.redo.push(entry);
            snapshot
        };
        self.source = snapshot.source;
        self.pages = snapshot.pages;
        self.history.content_revision = Some(snapshot.content_revision);
        self.revision = self.revision.wrapping_add(1);
        self.confirm_reload = false;
        self.status = if self.dirty() {
            "已恢复历史内容；草稿未保存"
        } else {
            "已恢复到保存内容"
        }
        .into();
        Some(snapshot.selection)
    }
}
