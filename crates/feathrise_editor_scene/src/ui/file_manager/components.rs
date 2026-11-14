//! 文件窗口固定实体池的身份标记；可复用行始终核对窗口与路径修订。

use super::types::FileAction;
use bevy::prelude::*;
use std::path::PathBuf;

#[derive(Component)]
pub(crate) struct FileWindowRoot { pub(crate) id: u64 }
#[derive(Component)]
pub(crate) struct FileTitleBar { pub(crate) window: u64 }
#[derive(Component)]
pub(crate) struct FileResizeHandle { pub(crate) window: u64, pub(crate) edges: u8 }
#[derive(Component, Clone, Copy)]
pub(crate) struct FileManagerButton { pub(crate) window: Option<u64>, pub(crate) action: FileAction }
#[derive(Component)]
pub(crate) struct FileTaskButton { pub(crate) window: u64 }
#[derive(Component)]
pub(crate) struct FileRow { pub(crate) window: u64, pub(crate) slot: usize, pub(crate) index: Option<usize>, pub(crate) path: Option<PathBuf>, pub(super) revision: u64 }
#[derive(Component)]
pub(crate) struct PathInput { pub(crate) window: u64 }
#[derive(Component)]
pub(crate) struct FileSearchInput { pub(crate) window: u64 }
#[derive(Component)]
pub(crate) struct FileNameInput;
#[derive(Component)]
pub(super) struct DesktopTaskbar;
#[derive(Component)]
pub(super) struct NameDialog;
#[derive(Component)]
pub(super) struct NameDialogBackdrop;
#[derive(Component)]
pub(super) struct NameDialogTitle;
#[derive(Component)]
pub(super) struct NameDialogBody;
#[derive(Component)]
pub(super) struct NameDialogBodyText;
#[derive(Component)]
pub(super) struct FileSearchHint { pub(super) window: u64 }
#[derive(Component)]
pub(super) struct DragLabel;
#[derive(Component)]
pub(super) struct WindowTitle(pub(super) u64);
#[derive(Component)]
pub(super) struct TaskTitle(pub(super) u64);
#[derive(Component)]
pub(super) struct WindowStatus(pub(super) u64);
#[derive(Component)]
pub(super) struct OperationStatus;
#[derive(Component, Clone, Copy)]
pub(super) struct ButtonLabel(pub(super) FileAction);
#[derive(Component)]
pub(super) struct RowLabel;
#[derive(Component)]
pub(super) struct FileRowIcon;
#[derive(Component)]
pub(crate) struct FileList { pub(crate) window: u64 }
#[derive(Component)]
pub(super) struct FileContextBackdrop;
#[derive(Component)]
pub(crate) struct FileContextPanel { pub(crate) submenu: bool }
#[derive(Component, Clone, Copy)]
pub(super) struct FileContextEntry { pub(super) submenu: bool, pub(super) action: super::types::FileContextAction }
#[derive(Component)]
pub(crate) struct FileContextLabel(pub(super) super::types::FileContextAction);
