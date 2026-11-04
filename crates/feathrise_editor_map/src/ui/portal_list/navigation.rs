//! 传送列表的页面可见性只依赖导航，不把Select中的Inspect误当作退出传送页。

use crate::resources::MapEditorState;
use crate::types::EditorTool;
use crate::ui::gameplay_sidebar::{GameplaySidebarPage, GameplaySidebarState};
use crate::ui::sidebars::{EditorLeftSidebarTab, MapEditorSidebarState};
use crate::ui::scene_inspector::{SelectedToolProperties, SelectedToolPropertiesKind};
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

#[derive(SystemParam)]
pub(crate) struct PortalSidebarNavigation<'w, 's> {
    state: Option<Res<'w, GameplaySidebarState>>,
    sidebar: Option<Res<'w, MapEditorSidebarState>>,
    forms: Query<'w, 's, &'static SelectedToolProperties>,
}

impl PortalSidebarNavigation<'_, '_> {
    pub(crate) fn visible(&self, editor: &MapEditorState) -> bool {
        if editor.open && !editor.map_transition_in_progress
            && self.sidebar.as_ref().is_some_and(|sidebar| sidebar.right == crate::ui::sidebars::EditorRightSidebarTab::Scene)
            && self.forms.iter().any(|form| form.shown_in_inspector(SelectedToolPropertiesKind::Portal))
        {
            return true; // 原传送表单已移到属性页，输入和完整列表继续使用这一套实体。
        }
        editor.open
            && self
                .sidebar
                .as_ref()
                .is_none_or(|sidebar| sidebar.tool_page_visible(EditorLeftSidebarTab::Gameplay))
            && self.state.as_ref().map_or_else(
                || editor.tool == EditorTool::Portal, // 独立领域测试或尚未挂载新导航时保留原工具入口。
                |state| state.page == GameplaySidebarPage::Portal,
            )
    }
}
