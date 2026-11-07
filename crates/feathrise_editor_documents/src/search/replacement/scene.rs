//! 复用暖墨铜边的中央并列差异与原生替换表单；按钮保持紧凑，正文只有固定虚拟行。

use feathrise_editor_ui::metrics as ui_metrics;
use super::{components::*, constants::*};
use bevy::{
    prelude::*,
    text::FontSourceTemplate,
    ui::{FocusPolicy, RelativeCursorPosition},
};
use feathrise_editor_core::FONT_PATH;
use feathrise_editor_ui::colors;

fn button(label: &str, action: ReplaceAction) -> impl Scene {
    bsn! {
        Button template_value(action)
        Node { height: px(26), padding: UiRect::horizontal(px(6)), border: px(1), flex_shrink: 0.0,
            align_items: AlignItems::Center, justify_content: JustifyContent::Center }
        BackgroundColor(colors::SLOT_BG) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [(
            Text(label) feathrise_editor_ui::localization::label_scene(label) template_value(ReplaceLabel::Button(action)) template_value(FocusPolicy::Pass) TextLayout::no_wrap()
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) } TextColor(colors::TEXT_PRIMARY)
        )]
    }
}

pub(crate) fn sidebar() -> impl Scene {
    bsn! {
        Node { width: percent(100), flex_direction: FlexDirection::Column, row_gap: px(3) }
        Children [(
            Node { width: percent(100), column_gap: px(3), align_items: AlignItems::Center }
            Children [(
            Text("替换为") feathrise_editor_ui::localization::label_scene("替换为") template_value(FocusPolicy::Pass) TextLayout::no_wrap()
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) } TextColor(colors::TEXT_MUTED)
        ), (
            ReplaceField feathrise_editor_core::EditorTextInput
            template_value(feathrise_editor_ui::single_line_input("", REPLACEMENT_CHARS))
            bevy::text::EditableTextFilter::new(|ch| !ch.is_control()) bevy::text::TextCursorStyle::default()
            TextLayout::no_wrap() template_value(FocusPolicy::Block)
            Node { height: px(26), min_width: px(0), flex_grow: 1.0, flex_basis: px(0),
                padding: UiRect::horizontal(px(4)), border: px(1), overflow: Overflow::clip() }
            BackgroundColor(colors::INPUT_BG) BorderColor::all(colors::INPUT_ACTIVE_BORDER)
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) } TextColor(colors::TEXT_PRIMARY)
        ), (button("替换预览", ReplaceAction::Open))]
        ), (
            template_value(ReplaceLabel::Status) feathrise_editor_ui::PanelStatusText("跨文件替换") feathrise_editor_ui::feedback::GlobalPanelStatus Text("") template_value(FocusPolicy::Pass)
            Node { display: Display::None, width: percent(100), min_width: px(0), flex_shrink: 0.0 }
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) } TextColor(colors::TEXT_MUTED)
        )]
    }
}

pub(crate) fn panel() -> impl Scene {
    bsn! {
        ReplacePanel template_value(FocusPolicy::Block) ZIndex(20)
        Node { display: Display::None, position_type: PositionType::Absolute, left: px(0), top: px(0),
            width: percent(100), height: percent(100), padding: px(8), row_gap: px(6), flex_direction: FlexDirection::Column,
            overflow: Overflow::clip() }
        BackgroundColor(colors::PANEL_BG_SECTION)
        Children [
            (
                Node { width: percent(100), min_height: px(24), flex_shrink: 0.0, padding: UiRect::right(px(22)) }
                Children [(
                    template_value(ReplaceLabel::Summary) Text("跨文件替换预览") feathrise_editor_ui::localization::label_scene("跨文件替换预览") template_value(FocusPolicy::Pass)
                    Node { min_width: px(0), flex_grow: 1.0 }
                    TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_BODY) } TextColor(colors::TEXT_ACCENT)
                ), (feathrise_editor_ui::context_help::help_button("跨文件替换", "搜索完成后输入替换文字，打开中央预览；空文字表示删除。先更新并核对差异，再替换当前匹配、当前文件或勾选文件。操作只改草稿，写盘使用保存本批或保存全部草稿。达到搜索上限时只处理已列出的匹配，未列出内容保持。", FONT_PATH))]
            ),
            (
                Node { width: percent(100), column_gap: px(4), row_gap: px(4), flex_wrap: FlexWrap::Wrap, flex_shrink: 0.0 }
                Children [(button("更新预览", ReplaceAction::Generate)), (button("替换此处", ReplaceAction::ApplyHit)),
                    (button("替换当前文件", ReplaceAction::ApplyFile)), (button("替换选中文件", ReplaceAction::ApplyChecked)),
                    (button("打开文件", ReplaceAction::OpenFile)), (button("撤销本批", ReplaceAction::Undo)),
                    (button("重做本批", ReplaceAction::Redo)), (button("保存本批", ReplaceAction::SaveBatch)),
                    (button("关闭预览", ReplaceAction::Close))]
            ),
            (
                template_value(ReplaceLabel::Status) feathrise_editor_ui::PanelStatusText("跨文件替换") feathrise_editor_ui::feedback::GlobalPanelStatus Text("") template_value(FocusPolicy::Pass)
                Node { display: Display::None, width: percent(100), min_width: px(0), flex_shrink: 0.0 }
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) } TextColor(colors::TEXT_MUTED)
            ),
            (super::super::super::batch_save::report()),
            (
                Node { width: percent(100), flex_grow: 1.0, flex_basis: px(0), min_height: px(0), column_gap: px(8) }
                Children [(
                    Node { width: percent(28), min_width: px(100), max_width: px(260), flex_direction: FlexDirection::Column, row_gap: px(4) }
                    Children [(
                        Node { column_gap: px(4), flex_wrap: FlexWrap::Wrap, flex_shrink: 0.0 }
                        Children [(button("全选", ReplaceAction::SelectAll)), (button("清空", ReplaceAction::SelectNone))]
                    ), (
                        ReplaceFiles RelativeCursorPosition::default() ScrollPosition::default()
                        Node { width: percent(100), flex_grow: 1.0, min_height: px(0), max_height: px(420), overflow: Overflow::scroll_y(), border: px(1) }
                        BorderColor::all(colors::COPPER_BORDER_WEAK) BackgroundColor(colors::INPUT_BG)
                        Children [(
                            ReplaceFileContent
                            Node { width: percent(100), height: px(0), flex_shrink: 0.0 }
                        )]
                    )]
                ), (
                    Node { flex_grow: 1.0, flex_basis: px(0), min_width: px(0), min_height: px(0), flex_direction: FlexDirection::Column, row_gap: px(4) }
                    Children [(
                        template_value(ReplaceLabel::Path) Text("选择左侧文件") feathrise_editor_ui::localization::label_scene("选择左侧文件") template_value(FocusPolicy::Pass)
                        Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 } TextLayout::no_wrap()
                        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) } TextColor(colors::TEXT_ACCENT)
                    ), (
                        Node { width: percent(100), flex_shrink: 0.0 }
                        Children [(
                            Text("修改前（原行:列）") feathrise_editor_ui::localization::label_scene("修改前（原行:列）") template_value(FocusPolicy::Pass)
                            Node { width: percent(50) } TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) } TextColor(colors::TEXT_MUTED)
                        ), (
                            Text("修改后") feathrise_editor_ui::localization::label_scene("修改后") template_value(FocusPolicy::Pass)
                            Node { width: percent(50) } TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) } TextColor(colors::TEXT_PRIMARY)
                        )]
                    ), (
                        ReplaceDiff RelativeCursorPosition::default() ScrollPosition::default()
                        Node { width: percent(100), flex_grow: 1.0, min_height: px(0), max_height: px(930), overflow: Overflow::scroll_y(), border: px(1) }
                        BorderColor::all(colors::COPPER_BORDER_WEAK) BackgroundColor(colors::INPUT_BG)
                        Children [(
                            ReplaceDiffContent
                            Node { width: percent(100), height: px(0), flex_shrink: 0.0 }
                        )]
                    )]
                )]
            ),
        ]
    }
}

pub(crate) fn tab() -> impl Scene {
    bsn! {
        ReplaceTab
        Node { display: Display::None, column_gap: px(3), height: px(32), flex_shrink: 0.0 }
        Children [(button("替换预览", ReplaceAction::Activate)), (button("×", ReplaceAction::Close))]
    }
}
