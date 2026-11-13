//! 文件窗口的浏览快照与手势；所有路径保持真实磁盘身份，不把 GLB 子资产混为文件。

use bevy::prelude::*;
use std::{collections::BTreeSet, path::PathBuf, sync::Arc};

#[derive(Clone, Debug)]
pub(crate) struct FileEntry {
    pub(crate) path: PathBuf,
    pub(crate) name: String,
    pub(crate) directory: bool,
    pub(crate) bytes: u64,
}

pub(crate) struct FileWindow {
    pub(crate) id: u64,
    pub(crate) path: PathBuf,
    pub(crate) requested_path: PathBuf,
    pub(crate) rect: Rect,
    pub(crate) restored_rect: Rect,
    pub(crate) minimized: bool,
    pub(crate) maximized: bool,
    pub(crate) entries: Arc<[FileEntry]>,
    pub(crate) selection: BTreeSet<PathBuf>,
    pub(crate) history: Vec<PathBuf>,
    pub(crate) history_index: usize,
    pub(crate) first: usize,
    pub(crate) loading: bool,
    pub(crate) error: Option<String>,
    pub(crate) filter: String,
    pub(crate) revision: u64,
    pub(super) scanned_revision: u64,
    pub(super) anchor: Option<usize>,
    pub(super) last_click: Option<(PathBuf, f64)>,
}

impl FileWindow {
    pub(super) fn navigate(&mut self, path: PathBuf, history: bool) {
        if history {
            self.history.truncate(self.history_index + 1);
            if self.history.last() != Some(&path) { self.history.push(path.clone()); }
            if self.history.len() > 128 { self.history.remove(0); }
            self.history_index = self.history.len().saturating_sub(1);
        }
        self.requested_path = path;
        self.revision = self.revision.wrapping_add(1);
        self.loading = true;
        self.error = None;
        self.last_click = None;
    }

    pub(super) fn refresh(&mut self) { self.navigate(self.requested_path.clone(), false); }
    pub(super) fn visible_capacity(&self) -> usize { (((self.rect.height() - 142.).max(26.) / 26.).floor() as usize).clamp(1, 64) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FileAction {
    NewWindow, Copy, Cut, Paste, Delete, NewMenu, NewFolder, NewText, NewMarkdown, NewGlb, Rename,
    Back, Forward, Up, Refresh, Minimize, Maximize, Close,
    Confirm, Cancel, ConflictPolicy, CancelTask, CopyHere, MoveHere,
}

impl FileAction {
    pub(super) fn label(self, english: bool) -> &'static str {
        match (self, english) {
            (Self::NewWindow, false) => "新窗口", (Self::NewWindow, true) => "New window",
            (Self::Copy, false) => "复制", (Self::Copy, true) => "Copy",
            (Self::Cut, false) => "剪切", (Self::Cut, true) => "Cut",
            (Self::Paste, false) => "粘贴", (Self::Paste, true) => "Paste",
            (Self::Delete, false) => "删除", (Self::Delete, true) => "Delete",
            (Self::NewMenu, false) => "新建    ›", (Self::NewMenu, true) => "New    ›",
            (Self::NewFolder, false) => "新建文件夹", (Self::NewFolder, true) => "New folder",
            (Self::NewText, false) => "文本文档 (.txt)", (Self::NewText, true) => "Text document (.txt)",
            (Self::NewMarkdown, false) => "Markdown (.md)", (Self::NewMarkdown, true) => "Markdown (.md)",
            (Self::NewGlb, false) => "GLB 场景 (.glb)", (Self::NewGlb, true) => "GLB scene (.glb)",
            (Self::Rename, false) => "重命名", (Self::Rename, true) => "Rename",
            (Self::Back, _) => "←", (Self::Forward, _) => "→", (Self::Up, _) => "↑",
            (Self::Refresh, false) => "刷新", (Self::Refresh, true) => "Refresh",
            (Self::Minimize, _) => "—", (Self::Maximize, _) => "□", (Self::Close, _) => "×",
            (Self::Confirm, false) => "确认", (Self::Confirm, true) => "Confirm",
            (Self::Cancel, false) => "取消", (Self::Cancel, true) => "Cancel",
            (Self::ConflictPolicy, false) => "同名：保留两者", (Self::ConflictPolicy, true) => "Conflict: keep both",
            (Self::CancelTask, false) => "取消任务", (Self::CancelTask, true) => "Cancel task",
            (Self::CopyHere, false) => "复制到这里", (Self::CopyHere, true) => "Copy here",
            (Self::MoveHere, false) => "移动到这里", (Self::MoveHere, true) => "Move here",
        }
    }

    pub(super) fn creates_entry(self) -> bool { matches!(self, Self::NewFolder | Self::NewText | Self::NewMarkdown | Self::NewGlb) }
    pub(super) fn extension(self) -> Option<&'static str> {
        match self { Self::NewText => Some("txt"), Self::NewMarkdown => Some("md"), Self::NewGlb => Some("glb"), _ => None }
    }
}

pub(super) struct FileContextMenu {
    pub(super) target: super::messages::FileManagerContextRequest,
    pub(super) submenu: bool,
    pub(super) initialized: bool,
    pub(super) main_rect: Rect,
    pub(super) submenu_rect: Rect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExtraAction {
    CopyAbsolute, CopyRelative, Favorite, References,
    External(crate::ui::external_files::ExternalFileAction),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FileContextAction { File(FileAction), Extra(ExtraAction) }

pub(super) struct WindowGesture {
    pub(super) window: u64,
    pub(super) origin: Vec2,
    pub(super) rect: Rect,
    pub(super) edges: u8,
}

pub(super) struct FileDrag {
    pub(super) sources: Vec<PathBuf>,
    pub(super) origin: Vec2,
    pub(super) position: Vec2,
    pub(super) copy: bool,
    pub(super) destination: Option<PathBuf>,
    pub(super) dragging: bool,
}

pub(super) struct NamePrompt {
    pub(super) action: FileAction,
    pub(super) directory: PathBuf,
    pub(super) sources: Vec<PathBuf>,
    pub(super) value: String,
    pub(super) initialized: bool,
}
