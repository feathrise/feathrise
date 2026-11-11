//! 顶部工具页的输入收口与固定节点显隐；不触碰场景数据、后台结果或撤销历史。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::EditorButtonClick;
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::text::EditableText;

use super::{components::*, resources::BuildingSidebarPage};
use crate::resources::SceneEditorState;
use crate::types::EditorTool;
use crate::ui::building_presets::BuildingPresetEditorState;
use crate::ui::courtyard::{CourtyardNameInput, CourtyardUiState, CourtyardViewportMode};
use crate::ui::placement_transform::PlacementTransformInputState;
use crate::ui::sidebars::{EditorLeftSidebarTab, SceneEditorSidebarState};
use crate::ui::text_input::EditorTextInputGuard;
use crate::ui::theme as colors;
use crate::viewport::modes::{ViewportMode, ViewportModeState};

#[allow(clippy::too_many_arguments)]
pub(super) fn handle_tabs(
    tabs: Query<(&EditorButtonClick, &BuildingSidebarTab), Changed<EditorButtonClick>>,
    names: Query<(Entity, &EditableText), With<CourtyardNameInput>>,
    editor: Res<SceneEditorState>,
    sidebar: Res<SceneEditorSidebarState>,
    guard: Res<EditorTextInputGuard>,
    relocation: Res<PlacementTransformInputState>,
    mut focus: ResMut<InputFocus>,
    mut page: ResMut<BuildingSidebarPage>,
    mut building: ResMut<BuildingPresetEditorState>,
    mut courtyard: ResMut<CourtyardUiState>,
    mut mode: ResMut<ViewportModeState>,
) {
    if !editor.open
        || editor.close_requested
        || editor.new_scene_dialog_open
        || !sidebar.tool_page_visible(EditorLeftSidebarTab::Building)
    {
        return;
    }
    for (interaction, tab) in &tabs {
        if !interaction.is_clicked() || tab.0 == *page {
            continue;
        }
        let name = names
            .single()
            .ok()
            .filter(|(entity, _)| focus.get() == Some(*entity));
        if guard.block_shortcuts || name.is_some_and(|(_, input)| input.is_composing()) {
            courtyard.status = "请先确认输入法候选，再切换工具页".to_owned();
            building.report_status(courtyard.status.clone()); // 被阻止时仍在当前可见反馈区说明原因。
            continue;
        }
        if relocation.relocation_armed() {
            courtyard.status = "请先完成或取消当前实例移动，再切换工具页".to_owned();
            building.report_status(courtyard.status.clone());
            continue;
        }
        if let Some((_, input)) = name {
            courtyard.name = input.value().to_string();
            focus.clear();
        }
        // 复用原取消/选择模式门，隐藏工具不能在下一次视口点击突然落印；选集和参数仍保留。
        let had_tool = courtyard.mode != CourtyardViewportMode::Idle;
        courtyard.cancel_viewport_mode();
        if had_tool {
            courtyard.status = "已取消院落视口工具；已确认名称和完整选集仍保留".to_owned();
        }
        mode.mode = ViewportMode::Select;
        *page = tab.0;
    }
}

pub(super) fn sync_pages(
    editor: Res<SceneEditorState>,
    sidebar: Res<SceneEditorSidebarState>,
    courtyard: Res<CourtyardUiState>,
    mut page: ResMut<BuildingSidebarPage>,
    mut panels: Query<(&BuildingSidebarPanel, &mut Node)>,
    mut tabs: Query<(
        &BuildingSidebarTab,
        &UiInteraction,
        &mut BackgroundColor,
        &mut BorderColor,
    )>,
) {
    // 显式武装的实际工具优先于旧视图；全局“编辑”恢复单栋工具时不能留下隐藏的点放入口。
    if editor.open && sidebar.tool_page_visible(EditorLeftSidebarTab::Building) {
        let active = if courtyard.mode != CourtyardViewportMode::Idle {
            Some(BuildingSidebarPage::Courtyard)
        } else if editor.tool == EditorTool::Building {
            Some(BuildingSidebarPage::Single)
        } else {
            None
        };
        if let Some(active) = active
            && *page != active
        {
            *page = active;
        }
    }
    for (panel, mut node) in &mut panels {
        let display = if panel.0 == *page {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
    for (tab, interaction, mut background, mut border) in &mut tabs {
        let selected = tab.0 == *page;
        let fill = match (*interaction, selected) {
            (UiInteraction::Pressed, _) => colors::SELECT_BG_PRESSED,
            (UiInteraction::Hovered, _) => colors::SELECT_BG_HOVER,
            (UiInteraction::None, true) => colors::SELECT_BG,
            (UiInteraction::None, false) => colors::BUTTON_DEV_SMALL_NORMAL,
        };
        let outline = BorderColor::all(if selected {
            colors::COPPER_BORDER
        } else {
            colors::COPPER_BORDER_WEAK
        });
        if background.0 != fill {
            background.0 = fill;
        }
        if *border != outline {
            *border = outline;
        }
    }
}
