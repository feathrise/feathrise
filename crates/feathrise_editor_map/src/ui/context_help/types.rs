//! 帮助来源及打开时的页面身份；异步回显或残留正文不能把浮层带到别的工作页。

use crate::ui::sidebars::{
    EditorLeftSidebarTab, EditorNavigatorTab, EditorRightSidebarTab, EditorSidebarHelp,
};
use crate::ui::vegetation_preset_editor::PresetWorkspaceTab;
use bevy::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum HelpTarget {
    Sidebar(EditorSidebarHelp),
    Section(Entity),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct HelpPage {
    pub navigator: EditorNavigatorTab,
    pub module: EditorLeftSidebarTab,
    pub right: EditorRightSidebarTab,
    pub workspace: Option<PresetWorkspaceTab>,
    pub map: Option<feathrise_map_content::MapId>,
    pub settings: Option<bool>, // 常规/快捷键是不同页面，切换时收回原帮助正文。
}
