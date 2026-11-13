//! 每个文件窗口独立保存浏览语义；目录任务仍共享两个在途槽，不复制资产加载器。

use super::super::types::{FileEntry, FileWindow};
use super::types::*;
use bevy::{prelude::*, tasks::Task};
use feathrise_editor_core::AssetBrowserViewMode;
use feathrise_editor_project_browser_model::{
    AssetTypeFilter, DirectoryAnimations, DirectoryWorker, EditorAssetEntry,
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
    pub(crate) fn service_open(&self, id: u64) -> bool {
        self.service.as_ref().is_some_and(|host| host.window == id)
            || self.popup.as_ref().is_some_and(|popup| popup.window == id)
            || self.release_window == Some(id)
    }

    pub(crate) fn capacity(&self, window: &FileWindow) -> usize {
        if self
            .sessions
            .get(&window.id)
            .is_some_and(|session| session.view == AssetBrowserViewMode::Thumbnails)
        {
            let layout =
                crate::ui::asset_thumbnails::grid_layout((window.rect.width() - 18.).max(96.), 0);
            let rows = ((window.rect.height() - 142.).max(26.)
                / (crate::ui::asset_thumbnails::TILE_HEIGHT
                    + crate::ui::asset_thumbnails::TILE_GAP))
                .floor()
                .max(1.) as usize;
            (layout.columns * rows).clamp(1, 12)
        } else {
            window.visible_capacity()
        }
    }

    pub(crate) fn service_host_active(&self) -> bool {
        self.service.is_some()
    }
    pub(crate) fn accepts_service_input(&self) -> bool {
        self.service.is_some() && self.service_input
    }
    pub(crate) fn accepts_service_mouse(&self) -> bool {
        self.service.is_some() && self.service_pointer
    }
    pub(crate) fn service_window(&self) -> Option<u64> {
        self.service.as_ref().map(|host| host.window)
    }
    pub(crate) fn search_window(&self) -> Option<u64> {
        self.service.as_ref().filter(|host| host.search).map(|host| host.window)
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
