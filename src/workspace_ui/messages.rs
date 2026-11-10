//! 工程页发出的低频意图；后台和持久化由宿主管理，UI不直接访问磁盘。

use super::WorkspaceField;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Message, Debug, Clone, Serialize, Deserialize)]
pub(crate) enum WorkspaceCommand {
    Open(PathBuf),
    Create {
        parent: PathBuf,
        name: String,
        #[serde(default)]
        game_view: feathrise_runtime::GameView,
    },
    CloseProject,
    QuitApp,
    SetStartupScene,
    ExportProject,
    OpenPlugins,
    Browse {
        field: WorkspaceField,
        initial: PathBuf,
    },
    SaveDefaultDirectory(String),
    RemoveRecent(PathBuf),
    CancelOperation,
}
