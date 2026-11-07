//! 撤销/重做采用紧凑铜边按钮；没有可用历史时禁用，不新增编辑缓冲。

use feathrise_editor_ui::metrics as ui_metrics;
use super::components::HistoryAction;
use bevy::{prelude::*, text::FontSourceTemplate, ui::FocusPolicy};
use feathrise_editor_core::FONT_PATH;
use feathrise_editor_ui::colors;

pub(crate) fn button(redo: bool) -> impl Scene {
    let action = if redo {
        HistoryAction::Redo
    } else {
        HistoryAction::Undo
    };
    let label = if redo { "重做" } else { "撤销" };
    bsn! {
        Button template_value(action)
        Node { display: Display::None, width: px(40), height: px(24), flex_shrink: 0.0,
            align_items: AlignItems::Center, justify_content: JustifyContent::Center,
            border: px(1), border_radius: BorderRadius::all(px(3)) }
        BackgroundColor(colors::SLOT_BG) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [(
            Text(label) feathrise_editor_ui::localization::label_scene(label) template_value(FocusPolicy::Pass)
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
            TextColor(colors::TEXT_PRIMARY)
        )]
    }
}
