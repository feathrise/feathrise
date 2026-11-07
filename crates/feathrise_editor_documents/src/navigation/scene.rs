//! 水墨铜木导航栏；侧栏和中央共用原生查找框，标记层完全穿透鼠标。

use feathrise_editor_ui::metrics as ui_metrics;
use super::{components::*, constants::*};
use bevy::{prelude::*, text::FontSourceTemplate, ui::FocusPolicy};
use feathrise_editor_core::FONT_PATH;
use feathrise_editor_ui::colors;

fn button(label: &str, action: NavigationAction) -> impl Scene {
    bsn! {
        Button template_value(action)
        Node { height: px(23), padding: UiRect::horizontal(px(5)), border: px(1), flex_shrink: 0.0,
            align_items: AlignItems::Center, justify_content: JustifyContent::Center }
        BackgroundColor(colors::SLOT_BG) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [(
            Text(label) feathrise_editor_ui::localization::label_scene(label) template_value(FocusPolicy::Pass) TextLayout::no_wrap()
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
            TextColor(colors::TEXT_PRIMARY)
        )]
    }
}

fn field(kind: NavigationField, max: usize) -> impl Scene {
    bsn! {
        template_value(kind) feathrise_editor_core::EditorTextInput
        template_value(feathrise_editor_ui::single_line_input("", max))
        bevy::text::EditableTextFilter::new(move |ch| !ch.is_control() && (kind != NavigationField::Line || ch.is_ascii_digit()))
        bevy::text::TextCursorStyle::default() TextLayout::no_wrap()
        template_value(FocusPolicy::Block)
        Node { height: px(24), min_width: px(0), flex_grow: 1.0, flex_basis: px(0),
            padding: UiRect::horizontal(px(4)), border: px(1), overflow: Overflow::clip() }
        BackgroundColor(colors::INPUT_BG) BorderColor::all(colors::INPUT_ACTIVE_BORDER)
        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
        TextColor(colors::TEXT_PRIMARY)
    }
}

pub(crate) fn tools() -> impl Scene {
    bsn! {
        Node { width: percent(100), flex_direction: FlexDirection::Column, row_gap: px(3), flex_shrink: 0.0 }
        Children [
            (
                NavigationTools
                Node { width: percent(100), column_gap: px(4), flex_shrink: 0.0 }
                Children [(button("查找 Ctrl+F", NavigationAction::Find)), (button("替换 Ctrl+H", NavigationAction::Replace)), (button("跳行 Ctrl+G", NavigationAction::Go))]
            ),
            (
                NavigationFindBar
                Node { display: Display::None, width: percent(100), flex_direction: FlexDirection::Column, row_gap: px(3) }
                Children [
                    (
                        Node { width: percent(100), column_gap: px(4) }
                        Children [(field(NavigationField::Query, QUERY_CHARS)), (button("关闭", NavigationAction::Close))]
                    ),
                    (
                        Node { width: percent(100), column_gap: px(4), align_items: AlignItems::Center }
                        Children [
                            (
                                NavigationCount Text("") template_value(FocusPolicy::Pass)
                                Node { min_width: px(0), flex_grow: 1.0 } TextLayout::no_wrap()
                                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                                TextColor(colors::TEXT_ACCENT)
                            ),
                            (button("上一个", NavigationAction::Previous)), (button("下一个", NavigationAction::Next)),
                        ]
                    ),
                    (
                        Node { width: percent(100), column_gap: px(4), flex_wrap: FlexWrap::Wrap, row_gap: px(3) }
                        Children [
                            (button("区分大小写", NavigationAction::CaseSensitive)),
                            (button("全词匹配", NavigationAction::WholeWord)),
                            (button("仅选区", NavigationAction::InSelection)),
                        ]
                    ),
                    (
                        NavigationReplaceBar
                        Node { display: Display::None, width: percent(100), column_gap: px(4), align_items: AlignItems::Center }
                        Children [
                            (
                                Text("替换为") feathrise_editor_ui::localization::label_scene("替换为") template_value(FocusPolicy::Pass) TextLayout::no_wrap()
                                Node { flex_shrink: 0.0 }
                                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
                                TextColor(colors::TEXT_MUTED)
                            ),
                            (field(NavigationField::Replacement, REPLACEMENT_CHARS)),
                            (button("替换当前", NavigationAction::ReplaceCurrent)),
                            (button("全部替换", NavigationAction::ReplaceAll)),
                        ]
                    ),
                    (
                        NavigationSearchHint feathrise_editor_ui::PanelStatusText("文档查找") feathrise_editor_ui::feedback::GlobalPanelStatus Text("") template_value(FocusPolicy::Pass)
                        Node { display: Display::None, width: percent(100), min_width: px(0) }
                        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                        TextColor(colors::TEXT_ACCENT)
                    ),
                ]
            ),
            (
                NavigationGoBar
                Node { display: Display::None, width: percent(100), flex_direction: FlexDirection::Column, row_gap: px(3) }
                Children [
                    (
                        Node { width: percent(100), column_gap: px(4) }
                        Children [(field(NavigationField::Line, 10)), (button("跳转", NavigationAction::SubmitLine)), (button("关闭", NavigationAction::Close))]
                    ),
                    (
                        NavigationLineHint feathrise_editor_ui::PanelStatusText("文档跳行") feathrise_editor_ui::feedback::GlobalPanelStatus Text("") template_value(FocusPolicy::Pass)
                        Node { display: Display::None }
                        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
                        TextColor(colors::TEXT_MUTED)
                    ),
                ]
            ),
        ]
    }
}

pub(crate) fn gutter() -> impl Scene {
    bsn! {
        NavigationGutter template_value(FocusPolicy::Pass)
        Node { width: px(GUTTER_WIDTH), min_height: px(0), flex_shrink: 0.0, overflow: Overflow::clip(), border: UiRect::right(px(1)) }
        BackgroundColor(colors::INPUT_BG) BorderColor::all(colors::COPPER_BORDER_WEAK)
    }
}

pub(crate) fn match_layer() -> impl Scene {
    bsn! {
        NavigationMatchLayer template_value(FocusPolicy::Pass)
        Node { position_type: PositionType::Absolute, left: px(4), top: px(4), right: px(4), bottom: px(4), overflow: Overflow::clip() }
        ZIndex(1)
    }
}
