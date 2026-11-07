//! 右侧所选地图元数据常驻页面；修改名称和备注继续通过唯一正式编辑弹框。

use super::{components::*, types::MapPropertiesField};
use crate::components::{MapEditorActionButton, MapEditorRightPanelScroll};
use crate::types::EditorAction;
use crate::ui::{scene::FONT_PATH, sidebars::*, theme as colors};
use bevy::prelude::*;
use bevy::text::{FontSourceTemplate, LineBreak};
use bevy::ui::{FocusPolicy, RelativeCursorPosition};
use feathrise_editor_ui::metrics;

fn field(label: &str, field: MapPropertiesField) -> impl Scene {
    bsn! {
        template_value(MapPropertiesRow(field))
        // 最小行高放在容器上：当前文本测量会把Text的min_height当成确定高度，导致换行后仍只占一行。
        Node { display: Display::Grid, width: percent(100), min_width: px(0), min_height: px(18), flex_shrink: 0.0,
            grid_template_columns: vec![RepeatedGridTrack::px(1, 46.0), RepeatedGridTrack::flex(1, 1.0)],
            column_gap: px(4), align_items: AlignItems::Start }
        Children [
            (Text(label) feathrise_editor_ui::localization::label_scene(label) template_value(FocusPolicy::Pass)
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(metrics::FONT_BODY) }
                TextColor(colors::TEXT_MUTED)
                Node { width: px(46), min_height: px(18), flex_shrink: 0.0 }),
            (template_value(MapPropertiesText(field)) Text("") template_value(FocusPolicy::Pass)
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(metrics::FONT_BODY) }
                TextLayout { linebreak: LineBreak::AnyCharacter } TextColor(colors::TEXT_VALUE)
                Node { width: percent(100), min_width: px(0) }),
        ]
    }
}

pub(crate) fn map_properties_page() -> impl Scene {
    bsn! {
        MapEditorSidebarPage(EditorSidebarTab::Right(EditorRightSidebarTab::Map))
        MapEditorRightPanelScroll RelativeCursorPosition::default() ScrollPosition::default()
        Node { display: Display::None, width: percent(100), min_width: px(0), min_height: px(0),
            flex_grow: 1.0, flex_basis: px(0), flex_direction: FlexDirection::Column,
            row_gap: px(4), padding: UiRect::top(px(3)), overflow: Overflow::scroll_y() }
        Children [
            (Node { width: percent(100), min_width: px(0), height: px(26), flex_shrink: 0.0,
                align_items: AlignItems::Center, justify_content: JustifyContent::SpaceBetween,
                border: UiRect::bottom(px(1)), margin: UiRect::bottom(px(2)) }
                BorderColor::all(colors::COPPER_BORDER_WEAK)
                Children [
                    (Text("地图") feathrise_editor_ui::localization::label_scene("地图") TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(metrics::FONT_BODY) }
                        TextColor(colors::TEXT_PRIMARY)),
                    (Button MapEditorActionButton(EditorAction::MapFolderRename) MapPropertiesEditBinding::default()
                        Node { width: px(46), height: px(24), align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center, border_radius: BorderRadius::all(px(2)) }
                        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL) BorderColor::all(colors::COPPER_BORDER_WEAK)
                        Children [(Text("编辑") feathrise_editor_ui::localization::label_scene("编辑") template_value(FocusPolicy::Pass)
                            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(metrics::FONT_CONTROL) }
                            TextColor(colors::TEXT_PRIMARY))]),
                ]),
            (field("名称", MapPropertiesField::Name)),
            (field("ID", MapPropertiesField::Id)),
            (field("类型", MapPropertiesField::Kind)),
            (field("备注", MapPropertiesField::Notes)),
            (field("场景", MapPropertiesField::Runtime)),
            (field("起始位置", MapPropertiesField::Spawn)),
            (field("清单", MapPropertiesField::Manifest)),
            (field("来源", MapPropertiesField::Source)),
        ]
    }
}
