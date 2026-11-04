//! 地表标签是稳定宿主构件；复杂正文在独立实现库构造。
use super::{components::*, resources::SurfacePresetEditorState, types::*};
use bevy::prelude::*;
use feathrise_editor_core::PresetWorkspaceTab;
use feathrise_editor_ui::workspace_tabs::spawn_preset_tab;

pub fn spawn_surface_tabs(
    tabs: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    state: &SurfacePresetEditorState,
    active_tab: PresetWorkspaceTab,
) {
    for tab in &state.tabs {
        let active = active_tab == PresetWorkspaceTab::Surface(tab.id);
        let dirty = if tab.draft.dirty() { "*" } else { "" };
        spawn_preset_tab(
            tabs,                                                          // 继续挂在共享中央标签条中。
            font,                                                          // 保留工作区中文字体。
            &format!("地表·{}{}", tab.draft.descriptor.label, dirty), // 类型、名称和未保存标记共同自然测量。
            SurfacePresetButton(SurfacePresetAction::ActivateTab(tab.id)), // 原有标签激活动作。
            SurfacePresetButton(SurfacePresetAction::CloseTab(tab.id)), // 原有关闭与确认事务。
            active,                 // 与其他类型保持同样的活动外框。
            tab.close_confirmation, // 确认文案不截断，也不跳过保护。
        );
    }
}
