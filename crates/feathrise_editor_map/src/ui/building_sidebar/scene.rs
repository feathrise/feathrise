//! 建筑与院落共用一个顶部导航及反馈宿主，正文保留各域唯一的原生控件。

use feathrise_editor_ui::metrics as ui_metrics;
use bevy::prelude::*;
use bevy::text::FontSourceTemplate;

use super::{components::*, resources::BuildingSidebarPage};
use crate::ui::building_presets::{
    BuildingPlacementControlText, BuildingPresetStatusText, building_preset_panel,
};
use crate::ui::courtyard::{CourtyardModeText, CourtyardStatusText, courtyard_panel};
use crate::ui::scene::FONT_PATH;
use crate::ui::theme as colors;

fn tab(page: BuildingSidebarPage) -> impl Scene {
    let label = page.label();
    bsn! {
        Button
        BuildingSidebarTab(page)
        Node {
            height: px(24),
            flex_grow: 1.0,
            flex_basis: px(0),
            min_width: px(0),
            border: UiRect::bottom(px(1)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
        }
        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL)
        BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [(
            Text(label)
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
            TextColor(colors::TEXT_PRIMARY)
        )]
    }
}

pub(crate) fn building_tools_panel() -> impl Scene {
    bsn! {
        BuildingSidebarRoot
        Node {
            width: percent(100),
            min_width: px(0),
            flex_direction: FlexDirection::Column,
            row_gap: px(4),
            flex_shrink: 0.0,
        }
        Children [
            (
                Node { width: percent(100), column_gap: px(4), flex_shrink: 0.0 }
                Children [
                    (tab(BuildingSidebarPage::Single)),
                    (tab(BuildingSidebarPage::Courtyard)),
                ]
            ),
            (
                BuildingSidebarPanel(BuildingSidebarPage::Single)
                Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }
                Children [(building_preset_panel())]
            ),
            (
                BuildingSidebarPanel(BuildingSidebarPage::Courtyard)
                Node { display: Display::None, width: percent(100), min_width: px(0), flex_shrink: 0.0 }
                Children [(courtyard_panel())]
            ),
            (
                // 切页只收起现有节点，名称输入、目录行与每域反馈都不会重建或被另一域覆盖。
                Node {
                    width: percent(100),
                    min_width: px(0),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(5),
                    flex_shrink: 0.0,
                }
                BorderColor::all(colors::COPPER_BORDER_WEAK)
                Children [
                    (
                        BuildingSidebarPanel(BuildingSidebarPage::Single)
                        Node { width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column, row_gap: px(4), flex_shrink: 0.0 }
                        Children [
                            (
                                BuildingPlacementControlText
                                Text("当前：选择 · 朝向 0°")
                                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                                TextColor(colors::TEXT_VALUE)
                                Node { width: percent(100), min_width: px(0) }
                            ),
                            (
                                BuildingPresetStatusText
                                template_value(crate::ui::status_bar::PanelStatusText("建筑工具"))
                                Text("")
                                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
                                TextColor(colors::TEXT_MUTED)
                                TextLayout { linebreak: LineBreak::AnyCharacter }
                                Node { width: percent(100), min_width: px(0) }
                            ),
                        ]
                    ),
                    (
                        BuildingSidebarPanel(BuildingSidebarPage::Courtyard)
                        Node { display: Display::None, width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column, row_gap: px(4), flex_shrink: 0.0 }
                        Children [
                            (
                                CourtyardModeText
                                Text("当前：普通选择 · 印章 0°")
                                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                                TextColor(colors::TEXT_VALUE)
                                Node { width: percent(100), min_width: px(0) }
                            ),
                            (
                                CourtyardStatusText
                                template_value(crate::ui::status_bar::PanelStatusText("院落工具"))
                                Text("")
                                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
                                TextColor(colors::TEXT_MUTED)
                                TextLayout { linebreak: LineBreak::AnyCharacter }
                                Node { width: percent(100), min_width: px(0) }
                            ),
                        ]
                    ),
                ]
            ),
        ]
    }
}
