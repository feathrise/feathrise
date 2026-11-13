//! 桌面文件窗口共用的后台文件事务契约；浏览会话关闭不代表撤回已经提交的整理任务。

use bevy::prelude::Resource;
use std::{path::PathBuf, sync::Arc};

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
    Copy { sources: Vec<PathBuf>, destination: PathBuf, conflict: ConflictPolicy },
    Move { sources: Vec<PathBuf>, destination: PathBuf, conflict: ConflictPolicy },
    /// 删除仅调用可恢复回收途径，不允许后台自行降级为永久删除。
    Delete { sources: Vec<PathBuf> },
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
}

#[derive(Clone, Debug, Default)]
pub struct FileOperationSnapshot {
    pub revision: u64,
    pub active: Option<FileOperationProgress>,
    pub queued: Vec<FileOperationProgress>,
    /// 最多保留最近32份回执；窗口应按id去重处理，而不能反复清空剪切板。
    pub completed: Vec<Arc<FileOperationReceipt>>,
}

pub trait FileOperationBackend: Send + Sync {
    fn enqueue(&self, request: FileOperationRequest) -> Result<u64, String>;
    /// 主帧不阻塞互斥锁；忙时保留上一快照，下一帧再采用完整状态。
    fn snapshot(&self) -> Option<FileOperationSnapshot>;
    fn cancel(&self, id: u64) -> bool;
    fn cancel_context(&self, context: u64);
    fn is_busy(&self) -> bool;
}

#[derive(Resource, Clone)]
pub struct FileOperationService(pub Arc<dyn FileOperationBackend>);

impl FileOperationService {
    pub fn enqueue(&self, request: FileOperationRequest) -> Result<u64, String> { self.0.enqueue(request) }
    pub fn snapshot(&self) -> Option<FileOperationSnapshot> { self.0.snapshot() }
    pub fn cancel(&self, id: u64) -> bool { self.0.cancel(id) }
    pub fn cancel_context(&self, context: u64) { self.0.cancel_context(context); }
    /// 退出/切工程的收口门；锁忙时也视为有任务，不能先关闭进程再遗留半份文件。
    pub fn is_busy(&self) -> bool { self.0.is_busy() }
}
