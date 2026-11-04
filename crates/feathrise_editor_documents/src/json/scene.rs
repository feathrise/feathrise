//! JSON诊断全文进入底栏，正文只保留有错误时可用的原定位动作。

use feathrise_editor_ui::metrics as ui_metrics;
use super::components::*;
use bevy::{prelude::*, text::FontSourceTemplate, ui::FocusPolicy};
use feathrise_editor_core::FONT_PATH;
use feathrise_editor_ui::colors;

pub(crate) fn format_button() -> impl Scene {
    bsn! {
        Button template_value(JsonAction::Format)
        Node { display: Display::None, width: px(52), height: px(24), flex_shrink: 0.0,
            align_items: AlignItems::Center, justify_content: JustifyContent::Center,
            border: px(1), border_radius: BorderRadius::all(px(3)) }
        BackgroundColor(colors::SLOT_BG) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [(
            Text("格式化") template_value(FocusPolicy::Pass)
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
            TextColor(colors::TEXT_PRIMARY)
        )]
    }
}

pub(crate) fn diagnostic() -> impl Scene {
    bsn! {
        Button template_value(JsonAction::Locate)
        Node { display: Display::None, height: px(24), min_width: px(80), flex_shrink: 0.0,
            align_items: AlignItems::Center, justify_content: JustifyContent::Center,
            padding: UiRect::horizontal(px(6)), border: px(1), border_radius: BorderRadius::all(px(3)) }
        BackgroundColor(colors::SLOT_BG) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [(
            DiagnosticLabel feathrise_editor_ui::PanelStatusText("JSON") feathrise_editor_ui::feedback::GlobalPanelStatus Text("") template_value(FocusPolicy::Pass)
            Node { display: Display::None, width: percent(100), min_width: px(0) }
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
            TextColor(colors::TEXT_HINT)
        ), (
            Text("定位错误") template_value(FocusPolicy::Pass) TextLayout::no_wrap()
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
            TextColor(colors::TEXT_ACCENT)
        )]
    }
}
