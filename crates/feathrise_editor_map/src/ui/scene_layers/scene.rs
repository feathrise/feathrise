//! 作者内容沿用正式主题和中文字体；完整列表使用十个槽连续滚动，参数仍只有右下方一份。

use feathrise_editor_ui::metrics as ui_metrics;
use super::{components::*, types::LayerList};
use crate::ui::{scene::FONT_PATH, theme as colors};
use bevy::{prelude::*, text::FontSourceTemplate};

fn row(list: LayerList, slot: usize) -> impl Scene {
    let icon = match list {
        LayerList::Water => "icons/editor/tree_water.png",
        LayerList::Vegetation => "icons/editor/tree_leaf.png",
        LayerList::DomainLayers | LayerList::DomainObjects => "icons/editor/tree_file.png",
    };
    bsn! { Button Row { list, slot } crate::ui::scene_context_menu::SceneObjectRow(None)
        Node { display: Display::None, position_type: PositionType::Absolute, width: percent(100),
            height: px(24), min_width: px(0), flex_shrink: 0.0, align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(20)), column_gap: px(5), border_radius: BorderRadius::all(px(2)), overflow: Overflow::clip_x() }
        BackgroundColor(colors::PANEL_BG) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [(crate::ui::scene::tree_icon(icon)),
            (Text("") TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
            TextColor(colors::TEXT_PRIMARY) TextLayout::no_wrap()
            template_value(bevy::ui::FocusPolicy::Pass) Node { min_width: px(0), flex_shrink: 0.0 })]
    }
}

fn section(list: LayerList) -> impl Scene {
    bsn! { Section(list) Node { display: Display::None, width: percent(100), min_width: px(0),
            flex_direction: FlexDirection::Column, row_gap: px(3), flex_shrink: 0.0 }
        Children [
            (Title(list) Text("") TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                TextColor(colors::TEXT_ACCENT) Node { width: percent(100), min_width: px(0), flex_shrink: 0.0,
                    padding: UiRect::top(px(4)), border: UiRect::bottom(px(1)) }
                BorderColor::all(colors::COPPER_BORDER_WEAK)),
            (List(list) crate::ui::list_layout::SceneObjectListScroll
                bevy::ui::RelativeCursorPosition::default() ScrollPosition::default()
                Node { width: percent(100), min_width: px(0), height: px(0), flex_shrink: 0.0, overflow: Overflow::scroll_y() }
                Children [(Content Node { width: percent(100), min_width: px(0), height: px(0), flex_shrink: 0.0 }
                    Children [(row(list, 0)), (row(list, 1)), (row(list, 2)), (row(list, 3)), (row(list, 4)),
                        (row(list, 5)), (row(list, 6)), (row(list, 7)), (row(list, 8)), (row(list, 9))])]),
        ]
    }
}

pub(crate) fn panel() -> impl Scene {
    bsn! { Node { width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column,
            flex_shrink: 0.0, row_gap: px(4) }
        Children [(section(LayerList::Water)), (section(LayerList::Vegetation)),
            (section(LayerList::DomainLayers)), (section(LayerList::DomainObjects))]
    }
}
