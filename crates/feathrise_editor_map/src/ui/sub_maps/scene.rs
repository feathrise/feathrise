//! 场景栏子地图分支使用固定行池和四个公共数值框，沿用铜边、米白与中文字体。

use feathrise_editor_ui::InteractionButton;
use feathrise_editor_ui::metrics as ui_metrics;
use bevy::{prelude::*, text::FontSourceTemplate};
use feathrise_editor_ui::{NumericInputSpec, NumericInputScrubConfig, NumericScrubFieldOptions,
    NumericScrubFieldStyle, NumericValue, spawn_numeric_scrub_field};
use super::components::*;
use crate::ui::{scene::FONT_PATH, theme as colors};

fn button(label: &str, action: Action) -> impl Scene {
    bsn! { InteractionButton ActionButton(action)
        Node { height: px(25), flex_grow: 1.0, min_width: px(0), border: px(1),
            border_radius: BorderRadius::all(px(3)), align_items: AlignItems::Center,
            justify_content: JustifyContent::Center, padding: UiRect::horizontal(px(3)) }
        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [Text(label) @feathrise_editor_ui::localization::label_scene(label) TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
            TextColor(colors::TEXT_PRIMARY)]
    }
}
fn row(index: usize) -> impl Scene {
    bsn! { InteractionButton Row(index)
        Node { display: Display::None, position_type: PositionType::Absolute, width: percent(100), height: px(24), flex_shrink: 0.0,
            align_items: AlignItems::Center, padding: UiRect::horizontal(px(4)), column_gap: px(5), border_radius: BorderRadius::all(px(2)) }
        BackgroundColor(colors::PANEL_BG) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [@crate::ui::scene::tree_icon("icons/editor/tree_map.png")--
            RowText(index) Text("") TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
            TextColor(colors::TEXT_VALUE) TextLayout::no_wrap()
            Node { flex_grow: 1.0, min_width: px(0), overflow: Overflow::clip_x() }]
    }
}
fn slot(index: usize) -> impl Scene {
    bsn! { Slot(index) Node { width: percent(100), min_width: px(0), height: px(24), flex_shrink: 0.0 } }
}
pub(crate) fn panel() -> impl Scene {
    bsn! { SceneBranch Node { display: Display::None, width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column,
            flex_shrink: 0.0, row_gap: px(3), padding: UiRect::top(px(4)) }
        Children [
            @crate::ui::context_help::section_header("子地图", "在父地图调整子地图引用的整体位置与朝向；内部内容请打开源地图编辑。复制或移除只操作当前引用，不修改源地图内容。")--
            Summary Text("子地图") @feathrise_editor_ui::localization::label_scene("子地图")
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
                TextColor(colors::TEXT_MUTED)--
            ListViewport crate::ui::list_layout::SceneObjectListScroll bevy::ui::RelativeCursorPosition::default() ScrollPosition::default()
                Node { width: percent(100), min_width: px(0), height: px(0), flex_shrink: 0.0, overflow: Overflow::scroll_y() }
                Children [ListContent Node { width: percent(100), min_width: px(0), height: px(0), flex_shrink: 0.0 }
                    Children [@row(0)-- @row(1)-- @row(2)-- @row(3)-- @row(4)-- @row(5)-- @row(6)-- @row(7)-- @row(8)-- @row(9)]]
        ]
    }
}
/// 创建入口归入顶部“玩法”，左侧场景树只显示真实存在的引用。
pub(crate) fn creation_button() -> impl Scene {
    bsn! { crate::components::MapEditorSceneContext Node { width: percent(100), flex_shrink: 0.0 }
        Children [@button("将目录所选地图添加为子地图", Action::Add)] }
}
pub(super) fn setup(mut commands: Commands, slots: Query<(Entity, &Slot), Added<Slot>>, server: Res<AssetServer>) {
    for (entity, slot) in &slots {
        let yaw = slot.0 == 3;
        let (min, max, step) = if yaw { (0.0, 360.0, 0.1) } else { (-1_000_000.0, 1_000_000.0, 0.001) };
        let spec = NumericInputSpec::f32(if yaw { 1 } else { 3 })
            .with_min(NumericValue::F32(min)).with_max(NumericValue::F32(max))
            .with_step(NumericValue::F32(step)).with_wrap(yaw).with_live_update(false)
            .with_step_buttons(false).with_replace_on_first_key(true);
        let options = NumericScrubFieldOptions::new(["位置 X", "位置 Y", "位置 Z", "朝向 Y"][slot.0],
            if yaw { "°" } else { "m" }, NumericValue::F32(0.0), spec,
            NumericInputScrubConfig::linear(if yaw { 0.5 } else { 0.1 }, step, min, max, 1.0))
            .with_style(NumericScrubFieldStyle { width: 164.0, height: 24.0, border_radius: 2.0,
                column_gap: 3.0, label_font_size: ui_metrics::FONT_HINT, value_font_size: ui_metrics::FONT_CONTROL, unit_font_size: 9.0 });
        commands.entity(entity).with_children(|parent| {
            let root = spawn_numeric_scrub_field(parent, Field(slot.0), feathrise_editor_core::editor_font(&server, FONT_PATH), options);
            parent.commands().entity(root).entry::<Node>().and_modify(|mut node| node.width = percent(100));
        });
    }
}

/// 所选属性只挂载在右侧；复用原控件与作者事务。
pub(crate) fn properties_panel() -> impl Scene {
    bsn! { ~{crate::ui::scene_inspector::InspectorSection::SubMap} Node { display: Display::None, width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column, row_gap: px(4), flex_shrink: 0.0 }
        Children [
            @slot(0)-- @slot(1)-- @slot(2)-- @slot(3)--
            Node { width: percent(100), column_gap: px(4) }
                Children [@button("复制引用", Action::Duplicate)-- @button("移除引用", Action::Remove)]--
            @button("打开源地图编辑", Action::OpenSource)
        ]
    }
}
