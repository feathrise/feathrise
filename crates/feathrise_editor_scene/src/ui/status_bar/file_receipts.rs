//! 文件任务详情只选择当前工程的回执，每次排版一页以复用原诊断正文。

use super::types::StatusAction;
use crate::ui::file_manager::FileManagerState;
use feathrise_editor_project_browser_model::file_operations::{FileOperationKind, FileOperationReceipt};
use feathrise_editor_ui::localization::EditorLocale;

pub(in crate::ui) const ITEMS_PER_PAGE: usize = 8;

#[derive(Default)]
pub(super) struct FileReceiptView {
    pinned: Option<u64>,
    page: usize,
    visible: Option<VisibleReceipt>,
    pub(super) text: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct VisibleReceipt {
    context: u64,
    id: u64,
    page: usize,
    pages: usize,
    earlier: Option<u64>,
    later: Option<u64>,
}

impl FileReceiptView {
    pub(super) fn observe(&mut self, files: Option<&FileManagerState>, locale: EditorLocale, locale_changed: bool) -> bool {
        let previous = self.visible;
        let current = files.and_then(|files| {
            let latest = files.receipts().next_back()?;
            if previous.is_none_or(|old| old.context != latest.context) {
                self.pinned = None;
            }
            let receipt = self.pinned.and_then(|id| files.receipts().find(|receipt| receipt.id == id))
                .unwrap_or(latest);
            // 回执按任务编号发布；历史淘汰后回到最新任务。只比较小队列的票据。
            self.pinned = (receipt.id != latest.id).then_some(receipt.id);
            let pages = item_count(receipt).div_ceil(ITEMS_PER_PAGE).max(1);
            self.page = if previous.is_some_and(|old| (old.context, old.id) == (receipt.context, receipt.id)) {
                self.page.min(pages - 1)
            } else {
                0
            };
            Some((receipt, VisibleReceipt {
                context: receipt.context, id: receipt.id, page: self.page, pages,
                earlier: files.receipts().rev().find(|item| item.id < receipt.id).map(|item| item.id),
                later: files.receipts().find(|item| item.id > receipt.id).map(|item| item.id),
            }))
        });
        self.visible = current.map(|(_, view)| view);
        if self.identity() == previous.map(VisibleReceipt::identity) && !locale_changed {
            return false;
        }
        self.text = current.map_or_else(String::new, |(receipt, view)| render(receipt, view.page, view.pages, locale));
        true
    }

    pub(super) fn identity(&self) -> Option<(u64, u64, usize)> {
        self.visible.map(VisibleReceipt::identity)
    }

    pub(super) fn can_navigate(&self, action: StatusAction) -> bool {
        self.visible.is_some_and(|view| match action {
            StatusAction::EarlierReceipt => view.earlier.is_some(),
            StatusAction::LaterReceipt => view.later.is_some(),
            StatusAction::PreviousReceiptPage => view.page > 0,
            StatusAction::NextReceiptPage => view.page + 1 < view.pages,
            _ => false,
        })
    }

    pub(super) fn has_navigation(&self) -> bool {
        self.visible.is_some_and(|view| view.pages > 1 || view.earlier.is_some() || view.later.is_some())
    }

    pub(super) fn navigate(&mut self, action: StatusAction) -> bool {
        let Some(view) = self.visible.filter(|_| self.can_navigate(action)) else { return false; };
        match action {
            StatusAction::EarlierReceipt => self.pinned = view.earlier,
            StatusAction::LaterReceipt => self.pinned = view.later,
            StatusAction::PreviousReceiptPage => self.page = view.page - 1,
            StatusAction::NextReceiptPage => self.page = view.page + 1,
            _ => return false,
        }
        true
    }
}

impl VisibleReceipt {
    fn identity(self) -> (u64, u64, usize) {
        (self.context, self.id, self.page)
    }
}

fn item_count(receipt: &FileOperationReceipt) -> usize {
    receipt.completed.len() + receipt.skipped.len() + usize::from(receipt.failed.is_some()) + receipt.unprocessed.len()
}

fn render(receipt: &FileOperationReceipt, page: usize, pages: usize, locale: EditorLocale) -> String {
    let action = match receipt.kind {
        FileOperationKind::Copy => "复制文件",
        FileOperationKind::Move => "移动文件",
        FileOperationKind::Delete => "移到回收站",
        FileOperationKind::CreateFolder => "新建文件夹",
        FileOperationKind::CreateFile => "新建文件",
        FileOperationKind::Rename => "重命名",
    };
    let mut text = locale.format("status.file_receipt.title", &[
        ("id", receipt.id.to_string()), ("action", locale.editor_label(action).into()),
        ("page", (page + 1).to_string()), ("pages", pages.to_string()),
    ]);
    text.push('\n');
    text.push_str(&locale.format("status.file_receipt.summary", &[
        ("completed", receipt.completed.len().to_string()), ("skipped", receipt.skipped.len().to_string()),
        ("unprocessed", receipt.unprocessed.len().to_string()),
    ]));
    if let Some(plan) = &receipt.reference_preview {
        text.push_str(&if locale == EditorLocale::English { format!("\n{} referencing documents found in preview", plan.documents.len()) }
            else { format!("\n引用预览涉及 {} 份场景或预设", plan.documents.len()) });
    } else if receipt.repaired_documents != 0 {
        text.push_str(&if locale == EditorLocale::English { format!("\nUpdated {} referencing documents", receipt.repaired_documents) }
            else { format!("\n已更新 {} 份引用文件", receipt.repaired_documents) });
    }
    if receipt.cancelled {
        text.push('\n');
        text.push_str(locale.text("status.file_receipt.cancelled"));
    }
    if let Some(error) = &receipt.error {
        text.push('\n');
        text.push_str(locale.text("status.file_receipt.failed"));
        text.push_str(" · ");
        text.push_str(&locale.editor_notice(error));
    }
    let start = page * ITEMS_PER_PAGE;
    for index in start..item_count(receipt).min(start + ITEMS_PER_PAGE) {
        let (label, path, target) = receipt_item(receipt, index, locale);
        text.push('\n');
        text.push_str(label);
        if let Some(path) = path {
            text.push_str(" · ");
            text.push_str(&crate::ui::external_files::platform_path(path).to_string_lossy());
        }
        if let Some(target) = target {
            text.push_str("\n  → ");
            text.push_str(&crate::ui::external_files::platform_path(target).to_string_lossy());
        }
    }
    text
}

fn receipt_item<'a>(
    receipt: &'a FileOperationReceipt, index: usize, locale: EditorLocale,
) -> (&'static str, Option<&'a std::path::Path>, Option<&'a std::path::Path>) {
    // 按各类条目计数直接定位起点。浏览大任务的最后一页同样只访问本页的条目。
    if let Some(change) = receipt.completed.get(index) {
        let label = if receipt.kind == FileOperationKind::Move && change.source_retained {
            "status.file_receipt.source_retained"
        } else {
            "status.file_receipt.completed"
        };
        let source = change.source.as_deref();
        let target = change.destination.as_deref();
        return (locale.text(label), source.or(target), source.and(target));
    }
    let index = index - receipt.completed.len();
    if let Some(path) = receipt.skipped.get(index) {
        return (locale.text("status.file_receipt.skipped"), Some(path), None);
    }
    let index = index - receipt.skipped.len();
    if index == 0 && let Some(path) = &receipt.failed {
        return (locale.text("status.file_receipt.failed"), Some(path), None);
    }
    let remaining = index - usize::from(receipt.failed.is_some());
    (locale.text("status.file_receipt.unprocessed"), receipt.unprocessed.get(remaining).map(|path| path.as_path()), None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use feathrise_editor_project_browser_model::file_operations::FileOperationChange;

    #[test]
    fn file_task_details_page_distinguishes_partial_move_failure_and_cancelled_items() {
        let mut receipt = FileOperationReceipt::new(7, 2, FileOperationKind::Move);
        receipt.completed.push(FileOperationChange {
            source: Some("original".into()), destination: Some("copied".into()), source_retained: true,
        });
        receipt.skipped.push("existing".into());
        receipt.failed = Some("failed-file".into());
        receipt.error = Some("错误原文".into());
        receipt.unprocessed.extend((0..ITEMS_PER_PAGE).map(|index| std::path::PathBuf::from(format!("remaining-{index}"))));
        receipt.cancelled = true;
        let first = render(&receipt, 0, 2, EditorLocale::English);
        assert!(first.contains("Copied · Source retained or partially retained · original"));
        assert!(first.contains("→ copied") && first.contains("Skipped · existing"));
        assert!(first.contains("Failed · failed-file") && first.contains("错误原文"));
        assert!(first.contains("Cancelled") && !first.contains("remaining-7"));

        let last = render(&receipt, 1, 2, EditorLocale::Chinese);
        assert!(last.contains("第 2/2 页") && last.contains("未处理 · remaining-7"));
        assert!(!last.contains("original") && !last.contains("existing") && !last.contains("failed-file"));
    }
}
