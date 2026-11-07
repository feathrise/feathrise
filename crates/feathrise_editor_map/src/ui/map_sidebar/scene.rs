//! 当前地图、虚拟目录树及所选对象按任务分区，保留原根落点和唯一虚拟行池。

use feathrise_editor_ui::metrics as ui_metrics;
use super::{components::*, types::SidebarField};
use crate::components::{
    MapEditorActionButton, MapEditorCustomActionStyle, MapEditorLeftPanelScroll, MapEditorMapList,
    MapEditorMapListTitle, MapEditorMapRootRow, MapEditorMapTreeScroll,
    MapEditorMapRowGlyph,
};
use crate::constants::MAP_BROWSER_ROW_HEIGHT;
use crate::types::EditorAction;
use crate::ui::scene::FONT_PATH;
use crate::ui::sidebars::{
    EditorNavigatorTab, EditorSidebarTab, EditorSidebarHelp, MapEditorSidebarHelpBody,
    MapEditorSidebarPage, sidebar_help_button,
};
use crate::ui::theme as colors;
use crate::ui::tooltips::{EditorTooltipTarget, TooltipAction};
use bevy::prelude::*;
use bevy::text::{FontSourceTemplate, LineBreak};
use bevy::ui::{FocusPolicy, RelativeCursorPosition};

fn field(field: SidebarField, initial: &str) -> impl Scene {
    let footer = field == SidebarField::SelectedSummary;
    let size = if footer { 12.0 } else { 10.5 };
    let border = if footer { UiRect::top(px(1)) } else { UiRect::ZERO };
    let padding = if footer { UiRect::top(px(6)) } else { UiRect::ZERO };
    let feedback = match field {
        SidebarField::DirectoryFeedback | SidebarField::OperationFeedback | SidebarField::CurrentState => Some("地图目录"),
        SidebarField::SelectedSummary => Some("地图目录选择"),
        _ => None,
    }.map(|label| bsn! { template_value(crate::ui::status_bar::PanelStatusText(label)) });
    (bsn! {
        template_value(MapSidebarText(field)) Text(initial) template_value(FocusPolicy::Pass)
        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(size) }
        TextLayout { linebreak: LineBreak::AnyCharacter } TextColor(colors::TEXT_VALUE)
        Node { width: percent(100), min_width: px(0), flex_shrink: 0.0, border: border, padding: padding }
        BorderColor::all(colors::COPPER_BORDER_WEAK)
    }, feedback)
}

/// 按钮继续走原EditorAction；只补显示目标绑定和本页独占的弱删除样式。
pub(super) fn action(label: &str, action: EditorAction) -> impl Scene {
    let button = bsn! {
        Button MapEditorActionButton(action) MapEditorCustomActionStyle
        template_value(MapSidebarActionBinding { menu_owned: true, ..default() })
        // 唯一五项操作迁入右键浮层，目录正文不再为常驻按钮预留一整行。
        Node { width: percent(100), height: px(26), min_width: px(0), flex_shrink: 0.0,
            border_radius: BorderRadius::all(px(2)), padding: UiRect::horizontal(px(8)),
            align_items: AlignItems::Center, justify_content: JustifyContent::FlexStart }
        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [(
            template_value(MapSidebarActionLabel(action)) Text(label) feathrise_editor_ui::localization::label_scene(label)
            template_value(FocusPolicy::Pass) TextLayout::no_wrap()
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
            TextColor(colors::TEXT_PRIMARY)
        )]
    };
    // 新建入口迁入目录后保留原悬停提示；附加同一按钮，不生成另一份操作实体。
    let tooltip = (action == EditorAction::NewMap)
        .then(|| bsn! { template_value(EditorTooltipTarget(TooltipAction::Editor(action))) });
    (button, tooltip)
}

/// 固定根目录继续承担选择上下文与地图拖回根级落点，不纳入滚动行池。
fn root_directory_row() -> impl Scene {
    bsn! {
        Button MapEditorMapRootRow
        Node { width: percent(100), min_width: px(0), height: px(MAP_BROWSER_ROW_HEIGHT),
            flex_shrink: 0.0, align_items: AlignItems::Center, padding: UiRect::horizontal(px(5)),
            column_gap: px(5), overflow: Overflow::clip_x() }
        BackgroundColor(colors::PANEL_BG) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [
            (template_value(MapEditorMapRowGlyph::Root) ImageNode::default() Node { width: px(18), height: px(18), flex_shrink: 0.0 } Pickable::IGNORE),
            (Text("根目录") feathrise_editor_ui::localization::label_scene("根目录") template_value(FocusPolicy::Pass) TextLayout::no_wrap()
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_BODY) }
            TextColor(colors::TEXT_VALUE) Node { min_width: px(0), overflow: Overflow::clip_x() }
            Pickable::IGNORE),
        ]
    }
}

pub(crate) fn map_page() -> impl Scene {
    bsn! {
        MapEditorSidebarPage(EditorSidebarTab::Navigator(EditorNavigatorTab::Maps))
        MapEditorLeftPanelScroll RelativeCursorPosition::default() ScrollPosition::default()
        Node { display: Display::Flex, width: percent(100), min_width: px(0), flex_grow: 1.0,
            min_height: px(0), flex_direction: FlexDirection::Column, row_gap: px(4),
            padding: UiRect::top(px(3)), overflow: Overflow::scroll_y() }
        Children [
            (field(SidebarField::DirectoryFeedback, "正在加载地图目录")),
            (root_directory_row()),
            (
                MapEditorMapTreeScroll RelativeCursorPosition::default() ScrollPosition::default()
                Node { width: percent(100), min_width: px(0), height: px(360), min_height: px(180),
                    max_height: px(520), flex_grow: 1.0, flex_shrink: 1.0, overflow: Overflow::scroll_y(),
                }
                Children [(
                    MapEditorMapList
                    Node { width: percent(100), min_width: px(0), height: px(0),
                        position_type: PositionType::Relative, flex_shrink: 0.0 }
                )]
            ),
            (field(SidebarField::SelectedSummary, "所选：尚未选择")),
            // 真实加载/保存/切图反馈保留唯一文字源，统一在底栏详情展示。
            (field(SidebarField::OperationFeedback, "")),
            (sidebar_help_button(EditorSidebarHelp::MapBrowser)),
            (
                MapEditorSidebarHelpBody(EditorSidebarHelp::MapBrowser)
                Node { display: Display::None, width: percent(100), min_width: px(0),
                    flex_direction: FlexDirection::Column, row_gap: px(3), flex_shrink: 0.0 }
                Children [
                    (field(SidebarField::CurrentSummary, "尚未进入地图")),
                    (MapEditorMapListTitle Text("地图 · 等待目录") feathrise_editor_ui::localization::label_scene("地图 · 等待目录") template_value(FocusPolicy::Pass)
                        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
                        TextLayout { linebreak: LineBreak::AnyCharacter } TextColor(colors::TEXT_MUTED)
                        Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }),
                    (field(SidebarField::CurrentName, "尚未进入地图")),
                    (field(SidebarField::CurrentState, "")),
                    (field(SidebarField::SelectedKind, "尚未选择")),
                    (field(SidebarField::SelectedName, "选择只改变目录高亮，不切换当前地图")),
                    (field(SidebarField::SelectedIdentity, "")),
                    (field(SidebarField::SelectionHint, "")),
                ]
            ),
        ]
    }
}
