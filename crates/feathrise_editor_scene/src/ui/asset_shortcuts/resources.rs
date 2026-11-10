//! 当前工作树的个人导航会话；一个 IO 槽涵盖载入、校验和原子保存。

use super::types::{Change, Completion, Section, Snapshot};
use bevy::{prelude::*, tasks::Task};
use std::path::PathBuf;

#[derive(Resource)]
pub(crate) struct AssetShortcuts {
    pub(super) root: PathBuf,
    pub(super) snapshot: Snapshot,
    pub(super) loaded: bool,
    pub(super) save_blocked: bool, // 损坏或不兼容记录不能被空默认值自动覆盖。
    pub(super) revision: u64,
    pub(super) saved_revision: u64,
    pub(super) failed_revision: Option<u64>,
    pub(super) task: Option<Task<Completion>>,
    pub(super) pending: Vec<Change>, // 首次载入期间重放真实点击，迟到磁盘快照不能吃掉收藏。
    pub(super) check_requested: bool,
    pub(super) was_open: bool,
    pub(super) expanded: [bool; 2],
    pub(super) feedback: String,
}

impl Default for AssetShortcuts {
    fn default() -> Self {
        Self {
            root: feathrise_editor_core::workspace_asset_root(), // 收藏与最近访问不读取另一工程同名路径。
            snapshot: default(),
            loaded: false,
            save_blocked: false,
            revision: 0,
            saved_revision: 0,
            failed_revision: None,
            task: None,
            pending: Vec::new(),
            check_requested: false,
            was_open: false,
            expanded: [true, false],
            feedback: String::new(),
        }
    }
}

impl AssetShortcuts {
    pub(crate) fn is_favorite(&self, path: &str, is_directory: bool) -> bool {
        let path = format!("assets/{path}");
        self.snapshot
            .favorites
            .iter()
            .any(|entry| entry.target.path == path && entry.target.is_directory == is_directory)
    }

    pub(super) fn entries(&self, section: Section) -> &[super::types::ShortcutEntry] {
        match section {
            Section::Favorites => &self.snapshot.favorites,
            Section::Recent => &self.snapshot.recent,
        }
    }

    pub(super) fn expanded(&self, section: Section) -> bool {
        self.expanded[usize::from(section == Section::Recent)]
    }
}
