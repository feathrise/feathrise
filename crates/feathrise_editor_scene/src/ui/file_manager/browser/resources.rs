//! 每个文件窗口独立保存浏览语义；目录任务仍共享两个在途槽，不复制资产加载器。

use super::super::types::{
    FILE_LIST_CHROME_HEIGHT, FILE_LIST_PADDING, FILE_ROW_SLOTS, FileEntry, FileWindow,
};
use super::types::*;
use crate::ui::asset_thumbnails::{TILE_GAP, TILE_HEIGHT};
use bevy::{prelude::*, tasks::Task};
use feathrise_editor_core::AssetBrowserViewMode;
use feathrise_editor_project_browser_model::{
    AnimationClipInfo, AssetTypeFilter, DirectoryAnimations, DirectoryWorker, EditorAssetEntry,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::Arc,
};

pub(in crate::ui::file_manager) struct BrowserSession {
    pub kind: AssetTypeFilter,
    pub view: AssetBrowserViewMode,
    pub worker: DirectoryWorker,
    pub root: PathBuf,
    pub assets: bool,
    pub entries: Arc<[EditorAssetEntry]>,
    pub animations: DirectoryAnimations,
    pub applied: u64,
    pub reveal: Option<(u64, String)>, // 收藏定位只随指定版本的目录请求发布，普通刷新不借用原文件。
}

impl BrowserSession {
    pub fn new(view: AssetBrowserViewMode) -> Self {
        Self {
            kind: AssetTypeFilter::All,
            view,
            worker: Default::default(),
            root: Default::default(),
            assets: false,
            entries: Default::default(),
            animations: Default::default(),
            applied: 0,
            reveal: None,
        }
    }
}

#[derive(Resource, Default)]
pub(crate) struct BrowserSessions {
    pub(in crate::ui::file_manager) asset_root: Option<PathBuf>,
    pub(in crate::ui::file_manager) sessions: BTreeMap<u64, BrowserSession>,
    pub(in crate::ui::file_manager) tasks: Vec<(u64, Task<BrowserScan>)>,
    pub(in crate::ui::file_manager) revealed: Option<(u64, u64, usize)>, // 窗口、目录修订和已验证行号仅送出一次属性意图。
    pub(super) popup: Option<BrowserPopup>,
    pub(super) service: Option<ServiceHost>,
    /// 仅原服务当前实际挂载的前景窗口可以借用总鼠标门；普通场景输入仍被阻断。
    pub(super) service_input: bool,
    pub(super) service_pointer: bool,
    pub(super) clip_intent: Option<(String, usize)>,
    pub(super) release_window: Option<u64>, // 菜单关闭按压及最终释放帧不能再点中其后的原文件行。
}

impl BrowserSessions {
    pub(in crate::ui::file_manager) fn picker_clip<'a>(&'a self, window: &FileWindow) -> Option<(&'a str, &'a AnimationClipInfo)> {
        let picker = window.animation_picker.as_ref()?;
        let index = window.anchor?;
        let file = window.entries.get(index)?;
        if window.loading || window.error.is_some() || file.directory || !window.selection.contains(&file.path) {
            return None;
        }
        let session = self.sessions.get(&window.id).filter(|session| session.applied == window.revision && session.assets)?;
        let path = &session.entries.get(index)?.asset_path;
        let clips = session.animations.get(path)?;
        let slot = picker.clip.or_else(|| (clips.len() == 1).then_some(0))?;
        Some((path, clips.get(slot)?))
    }

    pub(crate) fn service_open(&self, id: u64) -> bool {
        self.service.as_ref().is_some_and(|host| host.window == id)
            || self.popup.as_ref().is_some_and(|popup| popup.window == id)
            || self.release_window == Some(id)
    }

    pub(in crate::ui::file_manager) fn thumbnail_grid(&self, window: &FileWindow) -> Option<ThumbnailGrid> {
        self.sessions
            .get(&window.id)
            .filter(|session| session.view == AssetBrowserViewMode::Thumbnails)
            .map(|_| ThumbnailGrid::new(window.rect.size()))
    }

    pub(crate) fn accepts_service_input(&self) -> bool {
        self.service.is_some() && self.service_input
    }
    pub(crate) fn accepts_service_mouse(&self) -> bool {
        self.service.is_some() && self.service_pointer
    }
    pub(crate) fn shortcut_window(&self) -> Option<u64> {
        self.service.as_ref().filter(|host| !host.search).map(|host| host.window)
    }
    pub(crate) fn search_window(&self) -> Option<u64> {
        self.service.as_ref().filter(|host| host.search).map(|host| host.window)
    }
    pub(crate) fn shortcut_recent(&self) -> Option<bool> {
        self.service.as_ref().filter(|host| !host.search).map(|host| host.recent)
    }
    pub(crate) fn asset_path(&self, window: u64, index: usize) -> Option<&str> {
        let session = self
            .sessions
            .get(&window)
            .filter(|session| session.assets)?;
        let entry = session
            .entries
            .get(index)
            .filter(|entry| !entry.is_directory)?;
        Some(&entry.asset_path)
    }
    pub(crate) fn current_service_folder(
        &self,
        state: &super::super::resources::FileManagerState,
    ) -> Option<String> {
        let id = self.search_window()?;
        let session = self.sessions.get(&id).filter(|session| session.assets)?;
        let window = state.windows.iter().find(|window| window.id == id)?;
        let path = crate::ui::external_files::platform_path(&window.path);
        let root = crate::ui::external_files::platform_path(&session.root);
        Some(
            path.strip_prefix(root)
                .ok()?
                .to_string_lossy()
                .replace('\\', "/"),
        )
    }

    pub(crate) fn accepts_clip(&self, path: &str, index: usize) -> bool {
        self.clip_intent
            .as_ref()
            .is_some_and(|(wanted, wanted_index)| wanted == path && *wanted_index == index)
    }

    #[cfg(feature = "validation")]
    pub(crate) fn validation_view(&self, window: u64) -> Option<AssetBrowserViewMode> {
        self.sessions.get(&window).map(|session| session.view)
    }
}

#[derive(Clone, Copy)]
pub(in crate::ui::file_manager) struct ThumbnailGrid {
    pub width: f32,
    columns: usize,
    rows: usize,
}

impl ThumbnailGrid {
    const CARD_WIDTH: f32 = 136.;
    const ROW_PITCH: f32 = TILE_HEIGHT + TILE_GAP;

    fn new(size: Vec2) -> Self {
        let width = (size.x - 2. * (1. + FILE_LIST_PADDING)).max(1.); // 每侧 1px 窗框及正文内边距。
        let height = (size.y - FILE_LIST_CHROME_HEIGHT).max(0.);
        // 放宽窗口增加整列，剩余宽度留在右侧。整行分页也必须容纳在现有行池内。
        let columns = (((width + TILE_GAP) / (Self::CARD_WIDTH + TILE_GAP)) as usize)
            .clamp(1, FILE_ROW_SLOTS);
        let rows = (((height + TILE_GAP) / Self::ROW_PITCH) as usize)
            .clamp(1, FILE_ROW_SLOTS / columns);
        Self { width: width.min(Self::CARD_WIDTH), columns, rows }
    }

    pub fn offset(self, slot: usize) -> Vec2 {
        Vec2::new(
            FILE_LIST_PADDING + (slot % self.columns) as f32 * (self.width + TILE_GAP),
            (slot / self.columns) as f32 * Self::ROW_PITCH,
        )
    }

    pub fn capacity(self) -> usize { self.columns * self.rows }

    fn last_row(self, entry_count: usize) -> usize {
        entry_count.div_ceil(self.columns).saturating_sub(self.rows)
    }

    pub fn clamp_first(self, entry_count: usize, first: usize) -> usize {
        // 把收藏定位和缩放后的起点对齐到整行，最后一行可以不足一整列。
        (first / self.columns).min(self.last_row(entry_count)) * self.columns
    }

    pub fn scroll(self, window: &mut FileWindow, lines: f32, pixels: f32) {
        // 滚轮每刻度一行，触控板的小幅像素滚动累计到整行再重排。
        let delta = window.thumbnail_wheel + lines + pixels / Self::ROW_PITCH;
        if !delta.is_finite() { return; }
        let last_row = self.last_row(window.entries.len());
        let row = (window.first / self.columns).saturating_add_signed(delta as isize).min(last_row);
        window.first = row * self.columns;
        window.thumbnail_wheel = if (row == 0 && delta < 0.) || (row == last_row && delta > 0.) {
            0.
        } else {
            delta.fract()
        };
    }
}

pub(in crate::ui::file_manager) struct BrowserScan {
    pub window: u64,
    pub revision: u64,
    pub context: u64,
    pub requested: PathBuf,
    pub result: Result<BrowserSnapshot, String>,
}

pub(in crate::ui::file_manager) struct BrowserSnapshot {
    pub root: PathBuf,
    pub assets: bool,
    pub asset_folder: Option<String>,
    pub files: Arc<[FileEntry]>,
    pub entries: Arc<[EditorAssetEntry]>,
    pub animations: DirectoryAnimations,
    pub selection: BTreeSet<PathBuf>,
    pub reveal_index: Option<usize>,
}
