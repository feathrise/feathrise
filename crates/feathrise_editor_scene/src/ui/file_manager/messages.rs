//! 固定文件栏与浮动窗口使用同一命令入口，剪贴板与后台任务不依赖窗口存活。

use super::types::{FileAction, ProjectMotionTarget};
use bevy::prelude::*;
use std::path::PathBuf;

#[derive(Message)]
pub(crate) struct OpenFileWindow { pub(crate) path: PathBuf }
#[derive(Message)]
pub(crate) struct OpenAnimationPicker {
    pub(crate) context: u64,
    pub(crate) directory: PathBuf,
    pub(crate) target: ProjectMotionTarget,
}
#[derive(Message)]
pub(crate) struct OpenManagedFile { pub(crate) path: PathBuf }
#[derive(Message)]
pub(crate) struct FileManagerExternalDrag { pub(crate) sources: Vec<PathBuf> }
#[derive(Message)]
pub(crate) struct FileManagerCommand { pub(crate) action: FileAction, pub(crate) path: Option<PathBuf>, pub(crate) sources: Vec<PathBuf> }
/// 右键打开时就冻结目录与选择，菜单释放时不再读取可能已变化的固定行池。
#[derive(Message, Clone)]
pub(crate) struct FileManagerContextRequest {
    pub(crate) position: Vec2,
    pub(crate) directory: PathBuf,
    pub(crate) sources: Vec<PathBuf>,
    pub(crate) open_directory: PathBuf,
    pub(crate) is_directory: Option<bool>,
}

#[derive(Message)]
pub(crate) struct ExtraRequest {
    pub(crate) action: super::types::ExtraAction,
    pub(crate) target: FileManagerContextRequest,
}
