//! 单栋建筑工具页：预设库与摆放参数分区；实例变换仍由右侧原属性面板负责。

use feathrise_editor_ui::metrics as ui_metrics;
use bevy::prelude::*;
use bevy::text::FontSourceTemplate;

use super::components::{BuildingPlacementOptionText, BuildingSidebarActionButton};
use super::types::*;
use crate::ui::scene::FONT_PATH;
use crate::ui::theme as colors;

fn button(label: &str, action: BuildingPresetAction) -> impl Scene {
    bsn! {
        Button
        BuildingPresetButton(action)
        BuildingSidebarActionButton
        Node {
            height: px(24),
            flex_grow: 1.0,
            flex_basis: px(0),
            min_width: px(0),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(3)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            padding: UiRect::horizontal(px(5)),
            overflow: Overflow::clip_x(),
        }
        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL)
        BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [(
            BuildingPlacementOptionText(action)
            Text(label)
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
            TextColor(colors::TEXT_PRIMARY)
        )]
    }
}

fn section(label: &str) -> impl Scene {
    bsn! {
        Text(label)
        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
        TextColor(colors::TEXT_ACCENT)
        Node {
            width: percent(100),
            margin: UiRect::top(px(4)),
            padding: UiRect::top(px(6)),
            border: UiRect::top(px(1)),
        }
        BorderColor::all(colors::COPPER_BORDER_WEAK)
    }
}

pub(crate) fn building_preset_panel() -> impl Scene {
    bsn! {
        Node {
            width: percent(100),
            min_width: px(0),
            flex_direction: FlexDirection::Column,
            row_gap: px(5),
            flex_shrink: 0.0,
        }
        Children [
            (section("预设库")),
            (
                BuildingPresetSummaryText
                Text("已选建筑：尚未选择")
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                TextColor(colors::TEXT_VALUE)
                TextLayout { linebreak: LineBreak::AnyCharacter }
                Node { width: percent(100), min_width: px(0) }
            ),
            (button("从所选模型新建", BuildingPresetAction::CreateFromSelectedAsset)),
            (
                Node { width: percent(100), column_gap: px(4), flex_shrink: 0.0 }
                Children [
                    (button("编辑预设", BuildingPresetAction::EditSelected)),
                    (button("删除预设", BuildingPresetAction::DeleteSelected)),
                ]
            ),
            (
                BuildingPresetListTitle
                Text("建筑预设 · 等待目录")
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
                TextColor(colors::TEXT_MUTED)
                Node { width: percent(100), min_width: px(0) }
            ),
            (
                // 保留原三列目录和固定预算行池；顶部切页不销毁或重绑这些实体。
                BuildingPresetList
                Node {
                    width: percent(100),
                    min_width: px(0),
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: px(3),
                    row_gap: px(3),
                    flex_shrink: 0.0,
                }
            ),
            (crate::ui::context_help::section_header("摆放参数", "选择建筑预设后开始摆放；新建预设先在左侧资产选择GLB/GLTF。红色预览不可落点。单栋整平开启后会修改地形，并与建筑共用一次撤销。选择已有建筑后，在右侧属性中移动、复制或删除。")),
            (button("开始摆放", BuildingPresetAction::PlaceSelected)),
            (
                Node { width: percent(100), column_gap: px(4), flex_shrink: 0.0 }
                Children [
                    (button("左转 90°", BuildingPresetAction::RotateLeft)),
                    (button("右转 90°", BuildingPresetAction::RotateRight)),
                ]
            ),
            (button("网格：1.0 m · 切换", BuildingPresetAction::CycleGrid)),
            (button("连接吸附：关闭", BuildingPresetAction::ToggleConnectionSnap)),
            (button("单栋整平：关闭", BuildingPresetAction::ToggleTerrainLeveling)),
        ]
    }
}
