//! 管理弹窗的固定种类及短滚动会话身份；不保存作者数据或持久设置。

use crate::types::MapFolderDialogMode;
use feathrise_map_content::MapId;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum DialogKind {
    #[default]
    MapDeletion,
    Folder,
    TerrainConflict,
}

impl DialogKind {
    pub(super) fn index(self) -> usize {
        match self {
            Self::MapDeletion => 0,
            Self::Folder => 1,
            Self::TerrainConflict => 2,
        }
    }
}

#[derive(Default)]
pub(super) struct ScrollSessions {
    pub(super) deletion: Option<MapId>,
    pub(super) folder: Option<(MapFolderDialogMode, Option<String>)>,
    pub(super) terrain: Option<(MapId, [u8; 32])>,
}
