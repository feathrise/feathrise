//! 文档工具条与原生多行输入；正文在固定视区内滚动，不能撑开资产侧栏。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::InteractionButton;
use feathrise_editor_ui::metrics as ui_metrics;
use super::{components::*, constants::*};
use bevy::{
    prelude::*,
    text::FontSourceTemplate,
    ui::{FocusPolicy, RelativeCursorPosition},
};
use feathrise_editor_core::FONT_PATH;
use feathrise_editor_ui::colors;

fn button(label: &str, action: DocumentAction) -> impl Scene {
    bsn! {
        InteractionButton ~{action}
        Node { height: px(24), flex_grow: 1.0, min_width: px(0), flex_basis: px(0),
            align_items: AlignItems::Center, justify_content: JustifyContent::Center,
            border: px(1), border_radius: BorderRadius::all(px(3)) }
        BackgroundColor(colors::SLOT_BG) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [
            Text(label) @feathrise_editor_ui::localization::label_scene(label) ~{FocusPolicy::Pass}
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
            TextColor(colors::TEXT_PRIMARY)
        ]
    }
}

/// 图片/模型占用唯一侧栏画布时，仍允许显式打开所选文档的独立中央标签。
pub(crate) fn open_selected_button() -> impl Scene {
    button("在中央打开所选文档", DocumentAction::OpenSelected)
}

pub(crate) fn panel() -> impl Scene {
    bsn! {
        DocumentPanel
        Node { display: Display::None, position_type: PositionType::Absolute,
            left: px(0), top: px(0), width: percent(100), height: percent(100),
            padding: px(6), flex_direction: FlexDirection::Column, row_gap: px(5), overflow: Overflow::clip() }
        BackgroundColor(colors::PANEL_BG_SECTION)
        Children [

                Node { width: percent(100), column_gap: px(4), flex_shrink: 0.0, align_items: AlignItems::Center,
                    padding: UiRect::right(px(22)) }
                Children [

                        DocumentPathText Text("")
                        Node { flex_grow: 1.0, min_width: px(0), overflow: Overflow::clip_x() }
                        TextLayout::no_wrap()
                        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                        TextColor(colors::TEXT_ACCENT)
                    --

                        InteractionButton ~{DocumentAction::ToggleExpand}
                        Node { height: px(24), padding: UiRect::horizontal(px(6)), flex_shrink: 0.0,
                            align_items: AlignItems::Center, justify_content: JustifyContent::Center,
                            border: px(1), border_radius: BorderRadius::all(px(3)) }
                        BackgroundColor(colors::SLOT_BG) BorderColor::all(colors::COPPER_BORDER_WEAK)
                        Children [
                            DocumentExpandLabel Text("放大") @feathrise_editor_ui::localization::label_scene("放大") ~{FocusPolicy::Pass}
                            TextLayout::no_wrap()
                            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                            TextColor(colors::TEXT_PRIMARY)
                        ]
                    --
                    @feathrise_editor_ui::context_help::help_button("文档", "预览与编辑共用当前文件草稿。Ctrl+S保存当前文件，Ctrl+Shift+S保存全部草稿；重新载入前会保护未保存内容。Ctrl+F查找字面文字，Enter查找下一处，Shift+Enter查找上一处，可限制大小写、全词或当前选区。Ctrl+H替换只改草稿；Ctrl+G输入从1开始的逻辑行号后按Enter跳转，Esc关闭导航。加载、校验、保存结果和错误见底栏详情。", FONT_PATH)
                ]
            --

                Node { width: percent(100), column_gap: px(3), row_gap: px(3), flex_wrap: FlexWrap::Wrap, flex_shrink: 0.0 }
                Children [
                    @button("预览", DocumentAction::Preview)--
                    @button("编辑", DocumentAction::Edit)--
                    @button("保存", DocumentAction::Save)--
                    @super::batch_save::button()--

                        InteractionButton ~{DocumentAction::Reload}
                        Node { height: px(24), width: px(64), flex_shrink: 0.0,
                            align_items: AlignItems::Center, justify_content: JustifyContent::Center,
                            border: px(1), border_radius: BorderRadius::all(px(3)) }
                        BackgroundColor(colors::SLOT_BG) BorderColor::all(colors::COPPER_BORDER_WEAK)
                        Children [
                            DocumentReloadLabel Text("重新载入") @feathrise_editor_ui::localization::label_scene("重新载入") ~{FocusPolicy::Pass}
                            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                            TextColor(colors::TEXT_PRIMARY)
                        ]
                    --
                    @super::history::button(false)--
                    @super::history::button(true)--
                    @super::json::format_button()
                ]
            --
            @super::navigation::tools()--
            @super::json::diagnostic()--
            @super::batch_save::report()--

                DocumentExternalNotice
                Node { display: Display::None, width: percent(100), height: px(28), flex_shrink: 0.0,
                    column_gap: px(3), align_items: AlignItems::Center, overflow: Overflow::clip() }
                Children [

                        Text("磁盘已变化") @feathrise_editor_ui::localization::label_scene("磁盘已变化") TextLayout::no_wrap()
                        Node { flex_grow: 1.0, min_width: px(0), overflow: Overflow::clip_x() }
                        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                        TextColor(colors::TEXT_ACCENT)
                    --

                        InteractionButton ~{DocumentAction::KeepDraft}
                        Node { height: px(24), width: px(62), flex_shrink: 0.0, border: px(1),
                            align_items: AlignItems::Center, justify_content: JustifyContent::Center }
                        BackgroundColor(colors::SLOT_BG) BorderColor::all(colors::COPPER_BORDER_WEAK)
                        Children [
                            Text("保留草稿") @feathrise_editor_ui::localization::label_scene("保留草稿") ~{FocusPolicy::Pass} TextLayout::no_wrap()
                            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                            TextColor(colors::TEXT_PRIMARY)
                        ]
                    --

                        InteractionButton ~{DocumentAction::Reload}
                        Node { height: px(24), width: px(64), flex_shrink: 0.0, border: px(1),
                            align_items: AlignItems::Center, justify_content: JustifyContent::Center }
                        BackgroundColor(colors::SLOT_BG) BorderColor::all(colors::COPPER_BORDER_WEAK)
                        Children [
                            DocumentReloadLabel Text("重新载入") @feathrise_editor_ui::localization::label_scene("重新载入") ~{FocusPolicy::Pass} TextLayout::no_wrap()
                            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                            TextColor(colors::TEXT_PRIMARY)
                        ]

                ]
            --

                DocumentPreviewScroll RelativeCursorPosition::default() UiInteraction::default()
                ScrollPosition::default()
                Node { width: percent(100), flex_grow: 1.0, min_height: px(0), overflow: Overflow::scroll_y() }
                Children [
                    DocumentPreviewBody
                    Node { width: percent(100), flex_direction: FlexDirection::Column, row_gap: px(8),
                        flex_shrink: 0.0, padding: px(3) }
                ]
            --

                DocumentEditArea
                Node { display: Display::None, width: percent(100), flex_grow: 1.0, flex_basis: px(0),
                    min_height: px(0), min_width: px(0), column_gap: px(6) }
                Children [@super::navigation::gutter()--
                DocumentSource::default() feathrise_editor_core::EditorTextInput
                feathrise_editor_ui::EditorCodeSurface // 长源码沿用背景层级，搜索等短输入使用输入框层级。
                feathrise_editor_core::PresetNotesScroll // 复用官方文字局部滚动与输入法门。
                RelativeCursorPosition::default()
                bevy::text::EditableText { allow_newlines: true, max_characters: { Some(MAX_EDIT_CHARS) }, visible_lines: { None } }
                bevy::text::EditableTextFilter::new(|ch| ch == '\n' || ch == '\t' || !ch.is_control())
                bevy::text::TextCursorStyle::default()
                TextLayout::linebreak(bevy::text::LineBreak::WordOrCharacter)
                ~{FocusPolicy::Block}
                Node { display: Display::None, flex_grow: 1.0, flex_basis: px(0), min_width: px(0), min_height: px(0),
                    overflow: Overflow::clip(), padding: px(4), border: px(1) }
                BackgroundColor(colors::INPUT_BG) BorderColor::all(colors::INPUT_ACTIVE_BORDER)
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                TextColor(colors::TEXT_PRIMARY)
                Children [@super::navigation::match_layer()]
                -- @super::minimap::panel()]
            --

                DocumentStatus feathrise_editor_ui::PanelStatusText("文档") feathrise_editor_ui::feedback::GlobalPanelStatus Text("")
                Node { display: Display::None, width: percent(100), flex_shrink: 0.0 }
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
                TextColor(colors::TEXT_HINT)

        ]
    }
}

/// 中央宿主只接纳现有文档子树，不另建编辑器、解析任务或渲染相机。
pub(crate) fn center_host() -> impl Scene {
    bsn! {
        DocumentCenterHost
        Node { display: Display::None, position_type: PositionType::Relative,
            width: percent(100), flex_grow: 1.0, min_height: px(0), overflow: Overflow::clip() }
        BackgroundColor(colors::PANEL_BG_SECTION)
        Children [@super::search::replacement_panel()]
    }
}

/// 编辑器已移至中央时，侧栏保留明确入口，所选新文件追加独立标签。
pub(crate) fn sidebar_launch() -> impl Scene {
    bsn! {
        DocumentSidebarLaunch
        Node { display: Display::None, width: percent(100), flex_direction: FlexDirection::Column,
            padding: UiRect { left: px(6), right: px(28), top: px(6), bottom: px(6) } }
        Children [
            @feathrise_editor_ui::context_help::help_button("中央文档", "文档已在中央工作区展开。选择其他文档后，可用此按钮追加中央标签；已有文件的草稿和滚动位置继续保留。", FONT_PATH)--

                InteractionButton ~{DocumentAction::OpenSelected}
                Node { width: percent(100), height: px(28), min_height: px(28), flex_shrink: 0.0,
                    align_items: AlignItems::Center, justify_content: JustifyContent::Center,
                    border: px(1), border_radius: BorderRadius::all(px(3)) }
                BackgroundColor(colors::SLOT_BG) BorderColor::all(colors::COPPER_BORDER_WEAK)
                Children [
                    Text("在中央打开所选文档") @feathrise_editor_ui::localization::label_scene("在中央打开所选文档") ~{FocusPolicy::Pass}
                    TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                    TextColor(colors::TEXT_PRIMARY)
                ]

        ]
    }
}
