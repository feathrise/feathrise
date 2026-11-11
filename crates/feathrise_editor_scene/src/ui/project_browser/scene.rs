//! 单一工程目录的文件正文；进入资产子目录时由内部来源路由复用原资产行池及预览。

use feathrise_editor_ui::InteractionButton;
use super::{components::*, types::*};
use crate::{components::SceneEditorLeftPanelScroll, ui::{scene::FONT_PATH, theme as colors}};
use bevy::{prelude::*, text::FontSourceTemplate, ui::{FocusPolicy, RelativeCursorPosition}};
use feathrise_editor_ui::localization::label_scene;
use feathrise_editor_ui::metrics as ui_metrics;

fn action_button(label: &str, action: ProjectFileAction) -> impl Scene {
    bsn! {
        InteractionButton ~{ProjectFileActionButton(action)}
        Node { height: px(26), flex_grow: 1.0, flex_basis: px(0), min_width: px(0),
            align_items: AlignItems::Center, justify_content: JustifyContent::Center,
            padding: UiRect::horizontal(px(3)), border: px(1), border_radius: BorderRadius::all(px(3)) }
        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [
            Text(label) @label_scene(label) ~{FocusPolicy::Pass} TextLayout::no_wrap()
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
            TextColor(colors::TEXT_PRIMARY)
        ]
    }
}

pub(crate) fn files_panel() -> impl Scene {
    bsn! {
        ProjectBrowserContent(ProjectBrowserView::ProjectFiles)
        SceneEditorLeftPanelScroll RelativeCursorPosition::default() ScrollPosition::default()
        Node { display: Display::Flex, width: percent(100), min_width: px(0), min_height: px(0),
            flex_grow: 1.0, flex_direction: FlexDirection::Column, row_gap: px(4),
            padding: UiRect::top(px(3)), overflow: Overflow::scroll_y() }
        Children [
            ProjectFileFolderText Text("工程/") ~{FocusPolicy::Pass}
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                TextColor(colors::TEXT_VALUE) TextLayout::linebreak(bevy::text::LineBreak::AnyCharacter)
                Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }--
            Node { width: percent(100), height: px(26), min_width: px(0), flex_shrink: 0.0, column_gap: px(4) }
                Children [@action_button("上级", ProjectFileAction::Up)-- @action_button("刷新", ProjectFileAction::Refresh)]--
            Text("筛选当前目录文件名") @label_scene("筛选当前目录文件名") ~{FocusPolicy::Pass}
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
                TextColor(colors::TEXT_MUTED) Node { flex_shrink: 0.0 }--
            ProjectFileFilter crate::ui::text_input::EditorTextInput
                ~{feathrise_editor_ui::single_line_input("", 128)}
                bevy::text::EditableTextFilter::new(|character| !character.is_control())
                bevy::text::TextCursorStyle::default() TextLayout::no_wrap()
                Node { width: percent(100), height: px(26), min_width: px(0), flex_shrink: 0.0,
                    padding: UiRect::horizontal(px(4)), border: px(1), overflow: Overflow::clip() }
                BackgroundColor(colors::INPUT_BG) BorderColor::all(colors::COPPER_BORDER_WEAK)
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                TextColor(colors::TEXT_VALUE)--
            ProjectFileSummaryText Text("") ~{FocusPolicy::Pass}
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
                TextColor(colors::TEXT_MUTED) Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }--
            ProjectFileStatusText Text("尚未打开工程") ~{FocusPolicy::Pass}
                ~{crate::ui::status_bar::PanelStatusText("工程目录")} feathrise_editor_ui::feedback::GlobalPanelStatus
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
                TextColor(colors::TEXT_MUTED) TextLayout::linebreak(bevy::text::LineBreak::AnyCharacter)
                Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }--
            ProjectFileList::default() Node { width: percent(100), min_width: px(0), height: px(0), flex_shrink: 0.0,
                position_type: PositionType::Relative, overflow: Overflow::clip() }--
            ProjectFileSelectionText Text("双击文本文件打开；右键显示文件操作") ~{FocusPolicy::Pass}
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
                TextColor(colors::TEXT_VALUE) TextLayout::linebreak(bevy::text::LineBreak::AnyCharacter)
                Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }
        ]
    }
}
