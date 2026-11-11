//! 管理弹窗的固定种类及短滚动会话身份；不保存作者数据或持久设置。

use crate::types::SceneFolderDialogMode;
use feathrise_scene_content::SceneId;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum DialogKind {
    #[default]
    SceneDeletion,
    Folder,
    TerrainConflict,
}

impl DialogKind {
    pub(super) fn index(self) -> usize {
        match self {
            Self::SceneDeletion => 0,
            Self::Folder => 1,
            Self::TerrainConflict => 2,
        }
    }
}

#[derive(Default)]
pub(super) struct ScrollSessions {
    pub(super) deletion: Option<SceneId>,
    pub(super) folder: Option<(SceneFolderDialogMode, Option<String>)>,
    pub(super) terrain: Option<(SceneId, [u8; 32])>,
}
