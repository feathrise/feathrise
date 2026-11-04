//! 地图侧栏固定文字和带显示目标身份的原动作按钮。

use super::types::SidebarField;
use crate::types::{EditorAction, MapBrowserItem};
use bevy::prelude::*;

#[derive(Component, Clone, Copy, Default)]
pub(super) struct MapSidebarText(pub(super) SidebarField);

#[derive(Component, Clone, Copy)]
pub(super) struct MapSidebarActionLabel(pub(super) EditorAction);

impl Default for MapSidebarActionLabel {
    fn default() -> Self {
        Self(EditorAction::MapFolderCreate)
    }
}

/// 显示目标与实际目录选择必须相同，旧按下不能在切换选择后作用于另一个对象。
#[derive(Component, Clone, Default)]
pub(crate) struct MapSidebarActionBinding {
    pub(crate) selected: Option<MapBrowserItem>,
    pub(crate) enabled: bool,
    /// 菜单物理按下先冻结请求，待关闭和弦完全归还后才由原动作入口消费一次。
    pub(crate) menu_owned: bool,
    pub(crate) dispatched: bool,
}

impl MapSidebarActionBinding {
    pub(crate) fn matches(&self, selected: Option<&MapBrowserItem>) -> bool {
        self.enabled && self.selected.as_ref() == selected
            && (!self.menu_owned || self.dispatched)
    }
}

/// 右侧编辑按钮只消费最后呈现的单图身份，复用目录原编辑动作而不复制保存事务。
#[derive(Component, Clone, Default)]
pub(crate) struct MapPropertiesEditBinding {
    pub(crate) map_id: Option<feathrise_map_content::MapId>,
    pub(crate) enabled: bool,
}

#[derive(Component, Clone, Copy, Default)]
pub(super) struct MapPropertiesText(pub(super) super::types::MapPropertiesField);

#[derive(Component, Clone, Copy, Default)]
pub(super) struct MapPropertiesRow(pub(super) super::types::MapPropertiesField);
