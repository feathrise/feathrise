//! 保存入口保留原事务，报告唯一文字由两个宿主共享并统一交给底栏详情。

use feathrise_editor_ui::metrics as ui_metrics;
use super::{super::components::DocumentAction, components::*};
use bevy::{
    prelude::*,
    text::FontSourceTemplate,
    ui::{FocusPolicy, RelativeCursorPosition},
};
use feathrise_editor_core::FONT_PATH;
use feathrise_editor_ui::colors;

pub(crate) fn button() -> impl Scene {
    bsn! {
        Button template_value(DocumentAction::SaveAll)
        Node { height: px(24), padding: UiRect::horizontal(px(6)), flex_shrink: 0.0,
            align_items: AlignItems::Center, justify_content: JustifyContent::Center, border: px(1) }
        BackgroundColor(colors::SLOT_BG) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [(
            Text("保存全部草稿") feathrise_editor_ui::localization::label_scene("保存全部草稿") template_value(FocusPolicy::Pass) TextLayout::no_wrap()
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) } TextColor(colors::TEXT_PRIMARY)
        )]
    }
}

pub(crate) fn report() -> impl Scene {
    bsn! {
        SaveReport
        Node { display: Display::None, width: percent(100), min_width: px(0), flex_shrink: 0.0,
            flex_direction: FlexDirection::Column, padding: px(4), row_gap: px(3), border: px(1) }
        BackgroundColor(colors::INPUT_BG) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [(
            Node { width: percent(100), align_items: AlignItems::Center, column_gap: px(4) }
            Children [(
                SaveReportSummary feathrise_editor_ui::PanelStatusText("批量保存") feathrise_editor_ui::feedback::GlobalPanelStatus Text("") template_value(FocusPolicy::Pass)
                Node { display: Display::None, flex_grow: 1.0, min_width: px(0) }
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) } TextColor(colors::TEXT_ACCENT)
            ), (
                HideSaveReport Button
                Node { height: px(22), padding: UiRect::horizontal(px(5)), flex_shrink: 0.0,
                    align_items: AlignItems::Center, border: px(1) }
                BackgroundColor(colors::SLOT_BG) BorderColor::all(colors::COPPER_BORDER_WEAK)
                Children [(
                    Text("收起结果") feathrise_editor_ui::localization::label_scene("收起结果") template_value(FocusPolicy::Pass) TextLayout::no_wrap()
                    TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) } TextColor(colors::TEXT_PRIMARY)
                )]
            )]
        ), (
            SaveReportScroll RelativeCursorPosition::default() ScrollPosition::default()
            Node { width: percent(100), min_width: px(0), max_height: px(144), overflow: Overflow::scroll_y() }
            Children [(
                SaveReportBody feathrise_editor_ui::PanelStatusText("批量保存详情") feathrise_editor_ui::feedback::GlobalPanelStatus Text("") template_value(FocusPolicy::Pass)
                Node { display: Display::None, width: percent(100), min_width: px(0), flex_shrink: 0.0 }
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) } TextColor(colors::TEXT_PRIMARY)
            )]
        )]
    }
}
