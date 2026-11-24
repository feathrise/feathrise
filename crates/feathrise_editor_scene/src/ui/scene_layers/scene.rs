//! 作者内容沿用正式主题和中文字体；完整列表使用十个槽连续滚动，参数仍只有右下方一份。

use feathrise_editor_ui::InteractionButton;
use feathrise_editor_ui::metrics as ui_metrics;
use super::{components::*, types::{AuthorLayerCategory, LayerList}};
use crate::ui::scene_sidebar::{
    TREE_CAPTION_GAP, TREE_ICON_SIZE, TreeRowToggle, tree_object_branch_button,
    tree_row_icon_left, tree_section_header, tree_toggle_button, tree_toggle_icon,
};
use crate::ui::{scene::FONT_PATH, theme as colors};
use bevy::{prelude::*, text::FontSourceTemplate};

fn row(list: LayerList, slot: usize) -> impl Scene {
    let icon = list.author_category().map_or("icons/editor/tree_file.png", AuthorLayerCategory::icon);
    let toggles = TreeRowToggle::ALL.map(|toggle| row_toggle(list, toggle));
    let [participation, visibility, lock] = toggles;
    let in_section = matches!(list, LayerList::Water | LayerList::Vegetation);
    let left = tree_row_icon_left(usize::from(in_section), TREE_ICON_SIZE);
    let (right, gap) = if in_section { (4, 3.0) } else { (20, TREE_CAPTION_GAP) };
    bsn! { InteractionButton Row { list, slot } crate::ui::scene_context_menu::SceneObjectRow(None)
        Node { display: Display::None, position_type: PositionType::Absolute, width: percent(100),
            height: px(24), min_width: px(0), flex_shrink: 0.0, align_items: AlignItems::Center,
            padding: UiRect { left, right: px(right), ..UiRect::ZERO },
            column_gap: px(gap), border_radius: BorderRadius::all(px(2)), overflow: Overflow::clip_x() }
        BackgroundColor(colors::PANEL_BG) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [@row_lead(icon, gap)-- @row_label()--
            @participation-- @visibility-- @lock]
    }
}

fn row_lead(icon: &'static str, row_gap: f32) -> impl Scene {
    // 水域和植被叶子直接占下一层图标列，右侧按钮沿用原有间距。
    bsn! { Node { flex_shrink: 0.0, align_items: AlignItems::Center,
            padding: UiRect::right(px(TREE_CAPTION_GAP - row_gap)) } Pickable::IGNORE
        Children [@crate::ui::scene::tree_icon(icon)] }
}

fn row_label() -> impl Scene {
    // 长名称只在文字区内裁切，尾部三个图标继续可用。
    bsn! { RowLabel Pickable::IGNORE
        Node { flex_grow: 1.0, flex_basis: px(0), min_width: px(0), overflow: Overflow::clip_x() }
        Children [Text("") TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
            TextColor(colors::TEXT_PRIMARY) TextLayout::no_wrap() Pickable::IGNORE
            Node { width: percent(100), min_width: px(0) }] }
}

fn row_toggle(list: LayerList, toggle: TreeRowToggle) -> impl Scene {
    let water = (list == LayerList::Water).then(|| bsn! { ~{StillWaterRowToggle(toggle)} });
    let vegetation = (list == LayerList::Vegetation).then(|| bsn! { ~{VegetationRowToggle(toggle)} });
    matches!(list, LayerList::Water | LayerList::Vegetation).then(|| bsn! {
        @tree_toggle_button(toggle) @water @vegetation
        bevy::ui::InteractionDisabled Node { display: Display::None }
        Children [@tree_toggle_icon()]
    })
}

fn section_toggle(list: LayerList, toggle: TreeRowToggle) -> impl Scene {
    matches!(list, LayerList::Water | LayerList::Vegetation).then(|| bsn! {
        @tree_toggle_button(toggle) ~{SectionToggle { list, toggle, scene: None }}
        bevy::ui::InteractionDisabled Node { display: Display::None }
        Children [@tree_toggle_icon()]
    })
} // 等到分类状态成功绑定后才显示，避免先出现没有实际动作的控件。

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
    let category = list.author_category();
    let container = category.map(|category| section_container(list, category));
    let title = category.is_none().then(|| bsn! {
        Node { width: percent(100), min_width: px(0), padding: UiRect::top(px(4)),
            border: UiRect::bottom(px(1)), align_items: AlignItems::Center, flex_shrink: 0.0 }
        BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [@section_label(list)]
    });
    bsn! { @container @title }
}

fn section_container(list: LayerList, category: AuthorLayerCategory) -> impl Scene {
    let [participation, visibility, lock] = TreeRowToggle::ALL.map(|toggle| section_toggle(list, toggle));
    // 和 GLB 行一样，左侧按钮覆盖箭头及类型图标，行根接收名称正文的选择。
    bsn! {
        InteractionButton ~{SectionSelection(category)} crate::ui::scene_context_menu::SceneObjectRow(None)
        ~{tree_section_header()} BackgroundColor(colors::PANEL_BG) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [
            @tree_object_branch_button() ~{SectionArrow { list, scene: None }}
            Pickable { should_block_lower: false, is_hoverable: true }--
            Pickable::IGNORE
            Node { flex_grow: 1.0, flex_basis: px(0), min_width: px(0),
                align_items: AlignItems::Center, column_gap: px(TREE_CAPTION_GAP), overflow: Overflow::clip_x() }
            Children [@crate::ui::scene::tree_icon(category.icon())-- @section_label(list)]--
            Pickable::IGNORE Node { column_gap: px(3), flex_shrink: 0.0 }
            Children [@participation-- @visibility-- @lock]
        ]
    }
}

fn section_label(list: LayerList) -> impl Scene {
    // 分类文字占满正文的剩余空间，文本裁切时标题仍是一整块可点击区。
    bsn! {
        Node { flex_grow: 1.0, flex_basis: px(0), min_width: px(0), overflow: Overflow::clip_x() } Pickable::IGNORE
        Children [Title(list) Text("") TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
            TextColor(colors::TEXT_ACCENT) TextLayout::no_wrap() Pickable::IGNORE
            Node { width: percent(100), min_width: px(0) }]
    }
}

pub(crate) fn panel() -> impl Scene {
    bsn! { Node { width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column,
            flex_shrink: 0.0, row_gap: px(3) }
        Children [@section(LayerList::Water)-- @section(LayerList::Vegetation)--
            @section(LayerList::DomainLayers)-- @section(LayerList::DomainObjects)]
    }
}
