//! 地图目录菜单冻结打开时的完整来源与稳定目标，不借用随后换绑的行槽。

use crate::types::{EditorAction, MapBrowserIndexSignature, MapBrowserItem};
use bevy::prelude::*;
use feathrise_map_content::MapId;

#[derive(Clone)]
pub(super) struct DirectoryMenuTarget {
    pub selected: Option<MapBrowserItem>,
    pub projection: (MapBrowserIndexSignature, u64),
    pub map_id: Option<MapId>,
    /// 折叠文件夹也按完整树定位；每帧最多检查64项，找到后只看紧邻下一项深度。
    pub folder_empty: Option<bool>,
    pub scan_cursor: usize,
}

#[derive(Clone)]
pub(super) struct DirectoryMenuRequest {
    pub target: DirectoryMenuTarget,
    pub entity: Entity,
    pub action: EditorAction,
}
