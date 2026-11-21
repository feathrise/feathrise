//! 作者内容沿用正式主题和中文字体；完整列表使用十个槽连续滚动，参数仍只有右下方一份。

use feathrise_editor_ui::InteractionButton;
use feathrise_editor_ui::metrics as ui_metrics;
use super::{components::*, types::LayerList};
use crate::ui::scene_sidebar::{TreeRowToggle, tree_toggle_button, tree_toggle_icon};
use crate::ui::{scene::FONT_PATH, theme as colors};
use bevy::{prelude::*, text::FontSourceTemplate};

fn row(list: LayerList, slot: usize) -> impl Scene {
    let icon = match list {
        LayerList::Water => "icons/editor/tree_water.png",
        LayerList::Vegetation => "icons/editor/tree_leaf.png",
        LayerList::DomainLayers | LayerList::DomainObjects => "icons/editor/tree_file.png",
    };
    let toggles = TreeRowToggle::ALL.map(|toggle| vegetation_toggle(list, toggle));
    let [participation, visibility, lock] = toggles;
    let (inset, gap) = if list == LayerList::Vegetation { (4, 3) } else { (20, 5) };
    bsn! { InteractionButton Row { list, slot } crate::ui::scene_context_menu::SceneObjectRow(None)
        Node { display: Display::None, position_type: PositionType::Absolute, width: percent(100),
            height: px(24), min_width: px(0), flex_shrink: 0.0, align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(inset)), column_gap: px(gap), border_radius: BorderRadius::all(px(2)), overflow: Overflow::clip_x() }
        BackgroundColor(colors::PANEL_BG) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [@crate::ui::scene::tree_icon(icon)-- @row_label()--
            @participation-- @visibility-- @lock]
    }
}

fn row_label() -> impl Scene {
    // 长名称只在文字区内裁切，尾部三个图标继续可用。
    bsn! { RowLabel Pickable::IGNORE
        Node { flex_grow: 1.0, flex_basis: px(0), min_width: px(0), overflow: Overflow::clip_x() }
        Children [Text("") TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
            TextColor(colors::TEXT_PRIMARY) TextLayout::no_wrap() Pickable::IGNORE
            Node { width: percent(100), min_width: px(0) }] }
}

fn vegetation_toggle(list: LayerList, toggle: TreeRowToggle) -> impl Scene {
    (list == LayerList::Vegetation).then(|| bsn! {
        @tree_toggle_button(toggle) ~{VegetationRowToggle(toggle)}
        Children [@tree_toggle_icon()]
    })
}

fn section(list: LayerList) -> impl Scene {
    bsn! { Section(list) Node { display: Display::None, width: percent(100), min_width: px(0),
            flex_direction: FlexDirection::Column, row_gap: px(3), flex_shrink: 0.0 }
        Children [
            @section_title(list)--
            List(list) crate::ui::list_layout::SceneObjectListScroll
                bevy::ui::RelativeCursorPosition::default() ScrollPosition::default()
                Node { width: percent(100), min_width: px(0), height: px(0), flex_shrink: 0.0, overflow: Overflow::scroll_y() }
                Children [Content Node { width: percent(100), min_width: px(0), height: px(0), flex_shrink: 0.0 }
                    Children [@row(list, 0)-- @row(list, 1)-- @row(list, 2)-- @row(list, 3)-- @row(list, 4)--
                        @row(list, 5)-- @row(list, 6)-- @row(list, 7)-- @row(list, 8)-- @row(list, 9)]]
        ]
    }
}

fn section_title(list: LayerList) -> impl Scene {
    let arrow = matches!(list, LayerList::Water | LayerList::Vegetation).then(|| bsn! {
        InteractionButton ~{SectionArrow { list, scene: None }}
        Node { width: px(18), height: px(21), flex_shrink: 0.0,
            align_items: AlignItems::Center, justify_content: JustifyContent::Center }
        BackgroundColor(colors::PANEL_BG)
        Children [@tree_toggle_icon()]
    });
    // 分类文字填满箭头右侧的剩余空间，窄栏内裁切正文而不换行挤动下面的树项。
    bsn! { Node { width: percent(100), min_width: px(0), padding: UiRect::top(px(4)), border: UiRect::bottom(px(1)),
            align_items: AlignItems::Center, column_gap: px(3), flex_shrink: 0.0 }
        BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [
            @arrow--
            Node { flex_grow: 1.0, flex_basis: px(0), min_width: px(0), overflow: Overflow::clip_x() }
            Children [Title(list) Text("") TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                TextColor(colors::TEXT_ACCENT) TextLayout::no_wrap()
                Node { width: percent(100), min_width: px(0) }]
        ]
    }
}

pub(crate) fn panel() -> impl Scene {
    bsn! { Node { width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column,
            flex_shrink: 0.0, row_gap: px(4) }
        Children [@section(LayerList::Water)-- @section(LayerList::Vegetation)--
            @section(LayerList::DomainLayers)-- @section(LayerList::DomainObjects)]
    }
}
