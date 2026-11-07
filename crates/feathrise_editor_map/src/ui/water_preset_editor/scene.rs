//! 全局静水预设库与唯一反馈挂载点；中央水体文档和地图引用事务保持原入口。

use feathrise_editor_ui::metrics as ui_metrics;
use crate::ui::theme as colors;
use bevy::prelude::*;
use bevy::text::FontSourceTemplate;

use super::types::{
    WaterPresetAction, WaterPresetButton, WaterPresetList, WaterPresetListTitle,
    WaterPresetStatusText, WaterPresetSummaryText,
};
use crate::ui::scene::FONT_PATH;

fn button(label: &str, action: WaterPresetAction) -> impl Scene {
    bsn! {
        Button
        WaterPresetButton(action)
        Node {
            height: px(24), flex_grow: 1.0, flex_basis: px(0), min_width: px(0),
            border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(3)),
            align_items: AlignItems::Center, justify_content: JustifyContent::Center,
            padding: UiRect::horizontal(px(3)),
        }
        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL)
        BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [(
            Text(label) feathrise_editor_ui::localization::label_scene(label)
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(9.2) }
            TextColor(colors::TEXT_PRIMARY)
        )]
    }
}

/// 原领域仍负责预设保存与引用诊断，侧栏只把唯一实体移到统一反馈区。
pub(crate) fn water_preset_feedback() -> impl Scene {
    bsn! {
        WaterPresetStatusText
        template_value(crate::ui::status_bar::PanelStatusText("静水预设")) feathrise_editor_ui::feedback::GlobalPanelStatus
        Text("") // 教程归入作者工具的?帮助，保留唯一原反馈身份供底栏采集。
        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
        TextColor(colors::TEXT_MUTED)
        TextLayout { linebreak: LineBreak::AnyCharacter }
        Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }
    }
}

pub(crate) fn water_preset_panel() -> impl Scene {
    bsn! {
        Node {
            width: percent(100), flex_direction: FlexDirection::Column, row_gap: px(4),
            min_width: px(0), padding: UiRect::bottom(px(8)), border: UiRect::bottom(px(1)),
            flex_shrink: 0.0,
        }
        BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [
            (
                Text("全局静水预设库") feathrise_editor_ui::localization::label_scene("全局静水预设库")
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                TextColor(colors::TEXT_ACCENT)
                Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }
            ),
            (
                Node { width: percent(100), column_gap: px(3), flex_shrink: 0.0 }
                Children [
                    (button("新建预设", WaterPresetAction::Create)),
                    (button("编辑所选", WaterPresetAction::EditSelected)),
                    (button("删除预设", WaterPresetAction::DeleteSelected)),
                ]
            ),
            (button("应用到当前静水体", WaterPresetAction::ApplyToSelectedStillWater)),
            (
                WaterPresetSummaryText
                Text("当前预设：尚未选择") feathrise_editor_ui::localization::label_scene("当前预设：尚未选择")
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(9.0) }
                TextColor(colors::TEXT_VALUE)
                TextLayout { linebreak: LineBreak::AnyCharacter }
                Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }
            ),
            (
                WaterPresetListTitle
                Text("水体预设 · 等待目录") feathrise_editor_ui::localization::label_scene("水体预设 · 等待目录")
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(9.2) }
                TextColor(colors::TEXT_VALUE)
                TextLayout { linebreak: LineBreak::AnyCharacter }
                Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }
            ),
            (
                WaterPresetList
                Node {
                    width: percent(100), flex_wrap: FlexWrap::Wrap,
                    column_gap: px(3), row_gap: px(3), flex_shrink: 0.0,
                }
            ),
        ]
    }
}
