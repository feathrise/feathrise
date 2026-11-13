//! 收藏、访问记录与后台回执的纯数据；持久身份统一相对于当前项目。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ShortcutTarget {
    pub(crate) path: String, // 持久化为 assets/…，不保存工作树绝对路径或 glTF 子资产标签。
    pub(crate) is_directory: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ShortcutEntry {
    pub(super) target: ShortcutTarget,
    pub(super) available: bool, // 仅后台校验或真实目录条目提供存在证据。
}

#[derive(Clone, Default, Debug, PartialEq, Eq)]
pub(super) struct Snapshot {
    pub(super) favorites: Vec<ShortcutEntry>,
    pub(super) recent: Vec<ShortcutEntry>,
}

#[derive(Serialize, Deserialize)]
pub(super) struct SavedNavigation {
    pub(super) version: u32,
    pub(super) favorites: Vec<ShortcutTarget>,
    pub(super) recent: Vec<ShortcutTarget>,
}

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub(crate) enum Section {
    #[default]
    Favorites,
    Recent,
}

#[derive(Clone, Debug)]
pub(super) enum Change {
    Favorite(ShortcutTarget, bool), // 显式期望状态，迟到载入不能反转按钮上显示的“收藏”意图。
    Visit(ShortcutTarget),
    Remove(Section, ShortcutTarget),
    ClearRecent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum JobKind {
    Load,
    Save,
    Check,
}

pub(super) struct Job {
    pub(super) root: PathBuf,
    pub(super) revision: u64,
    pub(super) kind: JobKind,
    pub(super) snapshot: Snapshot,
}

pub(super) struct Completion {
    pub(super) root: PathBuf,
    pub(super) revision: u64,
    pub(super) kind: JobKind,
    pub(super) result: Result<Snapshot, String>,
}
