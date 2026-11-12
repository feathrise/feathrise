//! 资产和工程目录共用快照与后台扫描入口。每个原目录会话拥有自己的工作器。

use bevy::prelude::Resource;
use feathrise_scene_content::SceneAssetKind;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

/// 资产目录中的一个文件或子目录。
#[derive(Clone, Debug)]
pub struct EditorAssetEntry {
    /// 相对assets根的正斜杠路径。
    pub asset_path: String,
    /// UI显示文件名。
    pub display_name: String,
    /// 目录单独标记，点击时进入而不是选作画笔。
    pub is_directory: bool,
    /// 文件可被场景编辑器消费的用途。
    pub kind: SceneAssetKind,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AssetTypeFilter {
    #[default]
    All,
    Asset,
    Glb,
    Model,
    Animation,
    Skeleton,
    Image,
    Audio,
    Video,
}

impl AssetTypeFilter {
    pub fn needs_glb_metadata(self) -> bool {
        matches!(self, Self::Model | Self::Animation | Self::Skeleton)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DirectoryFilters {
    pub kind: AssetTypeFilter,
    pub filename: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnimationClipInfo {
    pub index: usize,
    pub label: String,
}

pub type DirectoryAnimations = HashMap<String, Arc<[AnimationClipInfo]>>;

#[derive(Debug, Default)]
pub struct DirectorySnapshot {
    pub entries: Vec<EditorAssetEntry>,
    pub animations: DirectoryAnimations,
}

impl DirectorySnapshot {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty() && self.animations.is_empty()
    }
}

/// 回收区只用于人工恢复，不得重新作为可摆放或可选择的正式作者资产出现。
pub fn hidden_author_directory(name: &str) -> bool {
    name == ".trash" // 当前只隐藏场景删除事务的唯一受控回收目录。
}

/// 两个浏览视图共用目录IO、路径边界、分类与排序；只由来源决定隐藏项和诊断名称。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FolderScope {
    Assets,
    ProjectFiles,
}

impl FolderScope {
    pub fn root_label(self) -> &'static str {
        match self {
            Self::Assets => "assets",
            Self::ProjectFiles => "工程",
        }
    }
    pub fn entry_label(self) -> &'static str {
        match self {
            Self::Assets => "资产",
            Self::ProjectFiles => "工程文件",
        }
    }
}

pub fn hidden_project_entry(name: &str) -> bool {
    // 发布暂存前缀全局保留；output只在工程根表示默认导出，不能隐藏src/output等作者目录。
    name.eq_ignore_ascii_case("target")
        || name.starts_with(".feathrise-export-")
        || name.eq_ignore_ascii_case(".git")
        || name.eq_ignore_ascii_case(".trash")
}

pub fn hidden_project_path(root: &Path, path: &Path) -> bool {
    path.strip_prefix(root).is_ok_and(|relative| {
        relative.components().enumerate().any(|(index, part)| {
            let name = part.as_os_str().to_string_lossy();
            hidden_project_entry(&name) || index == 0 && name.eq_ignore_ascii_case("output")
        })
    })
}

pub fn hidden_path(root: &Path, path: &Path, scope: FolderScope) -> bool {
    match scope {
        FolderScope::ProjectFiles => hidden_project_path(root, path),
        FolderScope::Assets => path.strip_prefix(root).is_ok_and(|relative| {
            relative
                .components()
                .any(|part| hidden_author_directory(&part.as_os_str().to_string_lossy()))
        }),
    }
}

pub struct DirectoryQuery<'a> {
    pub context: u64,
    pub scope: FolderScope,
    pub asset_root: Option<&'a Path>,
    pub filters: &'a DirectoryFilters,
    pub reveal_file: Option<&'a str>,
}

/// 真正的缓存、监听和目录IO由独立工程浏览库管理。
pub trait DirectoryBackend: Send + Sync {
    fn take_changed(&self) -> bool;
    fn request_refresh(&self);
    fn is_watching(&self) -> bool;
    fn clear(&self);
    fn scan(
        &self,
        root: PathBuf,
        folder: String,
        query: DirectoryQuery<'_>,
    ) -> Result<DirectorySnapshot, String>;
}

#[derive(Resource, Clone, Copy)]
pub struct DirectoryOperations {
    pub create: fn() -> Arc<dyn DirectoryBackend>,
}

/// 延迟到首个正式目录任务才分配后台实现；克隆只转交同一工作器的所有权。
#[derive(Clone, Default)]
pub struct DirectoryWorker(Option<Arc<dyn DirectoryBackend>>);

impl DirectoryWorker {
    pub fn ensure(&mut self, operations: DirectoryOperations) {
        self.0.get_or_insert_with(operations.create);
    }

    pub fn take_changed(&self) -> bool {
        self.0.as_ref().is_some_and(|worker| worker.take_changed())
    }

    pub fn request_refresh(&self) {
        if let Some(worker) = &self.0 {
            worker.request_refresh();
        }
    }

    pub fn is_watching(&self) -> bool {
        self.0.as_ref().is_some_and(|worker| worker.is_watching())
    }

    pub fn clear(&self) {
        if let Some(worker) = &self.0 {
            worker.clear();
        }
    }

    pub fn scan(
        &self,
        root: PathBuf,
        folder: String,
        query: DirectoryQuery<'_>,
    ) -> Result<DirectorySnapshot, String> {
        self.0
            .as_ref()
            .ok_or("工程目录扫描工作器尚未初始化")?
            .scan(root, folder, query)
    }
}
