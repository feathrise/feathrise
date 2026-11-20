//! 桌面文件窗口共用的后台文件事务契约；浏览会话关闭不代表撤回已经提交的整理任务。

use bevy::prelude::Resource;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
};
use crate::file_selection::FileSourceIdentity;
pub use crate::file_selection::{FileDirectory, FileEntry, FileEntryStamp, FileSources, path_key, platform_path};

/// 同名目标必须有明确策略；首版不提供覆盖，以免未保存文档和外部作者改动被静默替换。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ConflictPolicy {
    #[default]
    Fail,
    KeepBoth,
    Skip,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileOperationKind { Copy, Move, Delete, CreateFolder, CreateFile, Rename }

/// “新建”菜单只决定初始内容；完整文件名由作者确认，后台不隐式改名或追加扩展名。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NewFileKind { Text, Markdown, Glb }

#[derive(Clone, Debug)]
pub enum FileOperation {
    Copy { sources: FileSources, destination: PathBuf, conflict: ConflictPolicy },
    Move { sources: FileSources, destination: PathBuf, conflict: ConflictPolicy },
    /// 删除仅调用可恢复回收途径，不允许后台自行降级为永久删除。
    Delete { sources: FileSources },
    CreateFolder { parent: PathBuf, name: String },
    CreateFile { parent: PathBuf, name: String, kind: NewFileKind },
    Rename { source: PathBuf, name: String },
}

impl FileOperation {
    pub fn kind(&self) -> FileOperationKind {
        match self {
            Self::Copy { .. } => FileOperationKind::Copy,
            Self::Move { .. } => FileOperationKind::Move,
            Self::Delete { .. } => FileOperationKind::Delete,
            Self::CreateFolder { .. } => FileOperationKind::CreateFolder,
            Self::CreateFile { .. } => FileOperationKind::CreateFile,
            Self::Rename { .. } => FileOperationKind::Rename,
        }
    }

    fn source_paths(&self) -> impl Iterator<Item = &Path> {
        let (single, batch) = match self {
            Self::Copy { sources, .. } | Self::Move { sources, .. } | Self::Delete { sources } => (None, Some(sources)),
            Self::Rename { source, .. } => (Some(source.as_path()), None),
            _ => (None, None),
        };
        single.into_iter().chain(batch.into_iter().flat_map(FileSources::iter))
    }
}

#[derive(Clone, Debug)]
pub struct FileOperationRequest {
    /// 工程会话代次；回执不得被后来打开的另一工程当作自己的操作结果。
    pub context: u64,
    /// 仅包含用户已经在编辑器中打开的工程或目录根；后台再次核对真实路径及联接边界。
    pub allowed_roots: Vec<PathBuf>,
    pub operation: FileOperation,
}

#[derive(Clone, Debug)]
pub struct FileOperationProgress {
    pub id: u64,
    pub context: u64,
    pub label: String,
    pub completed: u64,
    pub bytes: u64,
    pub current: Option<PathBuf>,
    pub cancel_requested: bool,
}

#[derive(Clone, Debug)]
pub struct FileOperationChange {
    pub source: Option<PathBuf>,
    pub destination: Option<PathBuf>,
    /// 跨卷移动只完成复制或源部分保留时为true，文档桥不能据此把源身份改绑到目标。
    pub source_retained: bool,
}

#[derive(Clone, Debug)]
pub struct FileOperationReceipt {
    pub id: u64,
    pub context: u64,
    pub kind: FileOperationKind,
    /// 批量操作按顶层条目分别完成；取消或后续失败不伪装为整批原子回滚。
    pub completed: Vec<FileOperationChange>,
    pub skipped: Vec<PathBuf>,
    pub error: Option<String>,
    pub cancelled: bool,
    pub failed: Option<PathBuf>,
    pub unprocessed: Vec<PathBuf>,
    index: ReceiptIndex,
}

#[derive(Clone, Debug, Default)]
struct ReceiptIndex {
    parents: HashSet<PathBuf>,
    sources: HashMap<PathBuf, usize>,
    changed_paths: HashSet<PathBuf>,
    cut: Option<(FileSourceIdentity, FileSources)>,
}

impl Drop for FileOperationReceipt {
    fn drop(&mut self) {
        // 队列已淘汰的回执仍可能由详情持有，最后一个界面句柄只移交所有权。
        if let Some(pool) = bevy::tasks::AsyncComputeTaskPool::try_get()
            && (!self.completed.is_empty() || !self.skipped.is_empty() || !self.unprocessed.is_empty())
        {
            let data = (
                std::mem::take(&mut self.completed),
                std::mem::take(&mut self.skipped),
                std::mem::take(&mut self.unprocessed),
                std::mem::take(&mut self.index),
            );
            pool.spawn(async move { drop(data); }).detach();
        }
    }
}

impl FileOperationReceipt {
    pub fn new(id: u64, context: u64, kind: FileOperationKind) -> Self {
        Self { id, context, kind, completed: Vec::new(), skipped: Vec::new(), error: None,
            cancelled: false, failed: None, unprocessed: Vec::new(), index: ReceiptIndex::default() }
    }

    /// 只有后台完成时构建索引。帧系统查窗口路径和已打开文档，无需遍历整批明细。
    pub fn finish(&mut self, operation: &FileOperation, current: Option<PathBuf>) {
        if self.error.is_some() { self.failed = current; }
        let mut accounted = HashSet::new();
        for (index, change) in self.completed.iter().enumerate() {
            if let Some(source) = &change.source {
                if self.kind != FileOperationKind::Copy {
                    let key = path_key(source);
                    self.index.parents.extend(key.parent().map(Path::to_path_buf));
                    self.index.changed_paths.insert(key);
                    self.index.sources.insert(platform_path(source), index);
                }
                if self.cancelled || self.error.is_some() { accounted.insert(platform_path(source)); }
            }
            if let Some(destination) = &change.destination {
                let destination = path_key(destination);
                self.index.parents.extend(destination.parent().map(Path::to_path_buf));
                self.index.changed_paths.insert(destination);
            }
        }
        if self.cancelled || self.error.is_some() {
            accounted.extend(self.skipped.iter().map(|path| platform_path(path)));
            accounted.extend(self.failed.iter().map(|path| platform_path(path)));
            self.unprocessed = operation.source_paths()
                .filter(|path| !accounted.contains(&platform_path(path))).map(Path::to_path_buf).collect();
        }
        if let FileOperation::Move { sources, .. } = operation
            && self.completed.iter().any(|change| !change.source_retained)
        {
            let remaining = sources.retaining(|path| {
                self.change_for(path).is_none_or(|(change, _)| change.source_retained)
            });
            self.index.cut = Some((sources.identity(), remaining));
        }
    }

    pub fn affects_directory(&self, path: &Path) -> bool {
        let key = path_key(path);
        self.index.parents.contains(&key) || key.ancestors().any(|ancestor| self.index.changed_paths.contains(ancestor))
    }

    /// 返回实际改变的源项和相对于该源的原路径后缀，用于已打开文件的后续绑定。
    pub fn change_for(&self, path: &Path) -> Option<(&FileOperationChange, PathBuf)> {
        // 重绑采用查验后的物理路径，区分 Windows 大小写敏感目录中的独立文件。
        let physical = platform_path(path);
        let (source, index) = physical.ancestors().find_map(|source| {
            self.index.sources.get(source).map(|index| (source, *index))
        })?;
        Some((&self.completed[index], physical.strip_prefix(source).ok()?.to_path_buf()))
    }

    /// 批量交集只在文件后台计算。拖动和粘贴使用不同快照时按真实移动源核对。
    pub fn remaining_clipboard(&self, clipboard: &FileSources) -> Option<FileSources> {
        let (identity, remaining) = self.index.cut.as_ref()?;
        if identity.matches(clipboard) { return Some(remaining.clone()); }
        let remaining = clipboard.retaining(|path| self.change_for(path).is_none_or(|(change, _)| change.source_retained));
        (remaining.len() != clipboard.len()).then_some(remaining)
    }
}

#[derive(Clone, Debug)]
pub struct FileClipboard {
    pub context: u64,
    pub generation: u64,
    pub sources: FileSources,
    /// 此次复制或剪切已观察的最后一份文件回执；稍后到达的移动继续从选区剔除。
    pub through: u64,
}

#[derive(Clone, Debug, Default)]
pub struct FileOperationSnapshot {
    pub revision: u64,
    pub active: Option<FileOperationProgress>,
    pub queued: Vec<FileOperationProgress>,
    /// 最多保留最近32份回执；窗口应按id去重处理，而不能反复清空剪切板。
    pub completed: Vec<Arc<FileOperationReceipt>>,
    /// 当前选区结清此前移动后发布，可作为下一次粘贴的冻结起点。
    pub clipboard: Option<FileClipboard>,
}

pub trait FileOperationBackend: Send + Sync {
    fn enqueue(&self, request: FileOperationRequest) -> Result<u64, String>;
    /// 主帧不阻塞互斥锁；忙时保留上一快照，下一帧再采用完整状态。
    fn snapshot(&self) -> Option<FileOperationSnapshot>;
    fn cancel(&self, id: u64) -> bool;
    fn cancel_context(&self, context: u64);
    fn is_busy(&self) -> bool;
    fn publish_clipboard(&self, _clipboard: Option<FileClipboard>) -> bool { false }
}

#[derive(Resource, Clone)]
pub struct FileOperationService(pub Arc<dyn FileOperationBackend>);

impl FileOperationService {
    pub fn enqueue(&self, request: FileOperationRequest) -> Result<u64, String> { self.0.enqueue(request) }
    pub fn snapshot(&self) -> Option<FileOperationSnapshot> { self.0.snapshot() }
    pub fn cancel(&self, id: u64) -> bool { self.0.cancel(id) }
    pub fn cancel_context(&self, context: u64) { self.0.cancel_context(context); }
    pub fn publish_clipboard(&self, clipboard: Option<FileClipboard>) -> bool { self.0.publish_clipboard(clipboard) }
    /// 退出/切工程的收口门；锁忙时也视为有任务，不能先关闭进程再遗留半份文件。
    pub fn is_busy(&self) -> bool { self.0.is_busy() }
}
