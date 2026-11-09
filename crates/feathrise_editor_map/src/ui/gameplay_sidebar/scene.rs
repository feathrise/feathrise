//! 玩法工具页固定在唯一正文滚动区上方；三类原作者控件常驻且互斥显示。

use feathrise_editor_ui::InteractionButton;
use feathrise_editor_ui::metrics as ui_metrics;
use super::{components::*, types::*};
use crate::authoring::BoundaryThicknessInputSlot;
use crate::components::{MapEditorBoundaryText, MapEditorRightPanelScroll};
use crate::types::EditorAction;
use crate::ui::portal_scene::portal_authoring_panel;
use crate::ui::scene::{FONT_PATH, flex_editor_button};
use crate::ui::theme as colors;
use bevy::prelude::*;
use bevy::text::FontSourceTemplate;
use bevy::ui::RelativeCursorPosition;

fn tab(page: GameplaySidebarPage) -> impl Scene {
    let label = page.label();
    let action = page.action();
    bsn! {
        InteractionButton GameplaySidebarTab(action)
        Node {
            height: px(24), flex_grow: 1.0, flex_basis: px(0), min_width: px(0),
            border: UiRect::bottom(px(1)), align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
        }
        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL)
        BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [
            Text(label) @feathrise_editor_ui::localization::label_scene(label)
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
            TextColor(colors::TEXT_PRIMARY)
        ]
    }
}

fn heading(label: &str) -> impl Scene {
    bsn! {
        Text(label) @feathrise_editor_ui::localization::label_scene(label)
        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
        TextColor(colors::TEXT_ACCENT)
        TextLayout { linebreak: LineBreak::AnyCharacter }
        Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }
    }
}

fn feedback(kind: GameplaySidebarFeedbackKind) -> impl Scene {
    let node = bsn! {
        GameplaySidebarFeedbackText(kind) Text("")
        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
        TextColor(colors::TEXT_MUTED)
        TextLayout { linebreak: LineBreak::AnyCharacter }
        Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }
    };
    let source = match kind {
        GameplaySidebarFeedbackKind::Availability => Some("交互工具"),
        GameplaySidebarFeedbackKind::Message => Some("交互最近操作"),
        _ => None,
    };
    (node, source.map(|source| bsn! { ~{crate::ui::status_bar::PanelStatusText(source)} }))
}

fn browse(label: &str, action: GameplaySidebarBrowse) -> impl Scene {
    bsn! {
        InteractionButton GameplaySidebarBrowseButton(action)
        Node {
            width: percent(100), min_width: px(0), min_height: px(24),
            border: UiRect::all(px(1)), padding: UiRect::horizontal(px(5)),
            align_items: AlignItems::Center, justify_content: JustifyContent::Center,
            flex_shrink: 0.0,
        }
        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL)
        BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [
            Text(label) @feathrise_editor_ui::localization::label_scene(label)
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
            TextColor(colors::TEXT_PRIMARY)
        ]
    }
}

fn asset_panel() -> impl Scene {
    bsn! {
        GameplaySidebarPanel(GameplaySidebarPage::Asset)
        Node { width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column, row_gap: px(4), flex_shrink: 0.0 }
        Children [
            @crate::ui::context_help::section_header("资产摆放", "在左侧资产页选择文件，切到编辑模式后在中央地图点击摆放；落点沿用精确地面查询。摆放参数在视口工具条。选择已有实例后，在右侧属性中调整变换、复制或删除，资产选择独立保留。")--
            @feedback(GameplaySidebarFeedbackKind::Asset)--
            @browse("打开资产浏览", GameplaySidebarBrowse::Assets)--
            @browse("查看实例属性", GameplaySidebarBrowse::Scene)
        ]
    }
}

fn boundary_panel() -> impl Scene {
    bsn! {
        GameplaySidebarPanel(GameplaySidebarPage::Boundary)
        Node { display: Display::None, width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column, row_gap: px(4), flex_shrink: 0.0 }
        Children [
            ~{crate::ui::scene_inspector::SelectedToolProperties::new(crate::ui::scene_inspector::SelectedToolPropertiesKind::Boundary)}
            Node { width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column, row_gap: px(4), flex_shrink: 0.0 }
            Children [
            @heading("1 · 空气墙对象")--
            crate::authoring::boundary::objects::WallPanelSlot Node { width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column, row_gap: px(3), flex_shrink: 0.0 }-- // 原八行池、页码和单墙事务只有一个挂载槽。
            @crate::ui::context_help::section_header("2 · 路径编辑", "先选择或新建空气墙，切到编辑模式后拖控制点，Ctrl点击线段插点。开放墙左键继续加点，Delete删除明确选中的点。外框必须闭合，内部墙可开放或闭合。重画只影响所选墙。")--
            @flex_editor_button("重画所选墙", EditorAction::BoundaryRedraw)--

                Node { width: percent(100), min_width: px(0), column_gap: px(3), flex_shrink: 0.0 }
                Children [
                    @flex_editor_button("删除选中点", EditorAction::BoundaryRemoveLastPoint)--
                    @flex_editor_button("闭合 / 打开", EditorAction::BoundaryClose)
                ]
            --
            @heading("3 · 墙体参数")--

                BoundaryThicknessInputSlot
                Node { width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column, row_gap: px(3), flex_shrink: 0.0 } // 三个公共数值框纵排，原字段身份、实时拖动和撤销不变。
            --

                MapEditorBoundaryText Text("区域边界：等待地图作者数据") @feathrise_editor_ui::localization::label_scene("区域边界：等待地图作者数据")
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                TextColor(colors::TEXT_VALUE)
                TextLayout { linebreak: LineBreak::AnyCharacter }
                Node { width: percent(100), min_width: px(0), padding: UiRect::all(px(4)), border: UiRect::all(px(1)), flex_shrink: 0.0 }
                BackgroundColor(colors::SLOT_BG) BorderColor::all(colors::COPPER_BORDER_WEAK)

            ]
        ]
    }
}

pub(crate) fn gameplay_tools_panel() -> impl Scene {
    bsn! {
        GameplaySidebarRoot
        Node { width: percent(100), min_width: px(0), min_height: px(0), flex_grow: 1.0, flex_direction: FlexDirection::Column, row_gap: px(4) }
        Children [

                Node { width: percent(100), min_width: px(0), column_gap: px(3), flex_shrink: 0.0 }
                Children [@tab(GameplaySidebarPage::Asset)-- @tab(GameplaySidebarPage::Portal)-- @tab(GameplaySidebarPage::Boundary)]
            -- // 工具导航不参与正文滚动，低窗口仍能切页。

                GameplaySidebarScroll MapEditorRightPanelScroll RelativeCursorPosition::default() ScrollPosition::default()
                Node { width: percent(100), min_width: px(0), min_height: px(0), flex_grow: 1.0, flex_basis: px(0), flex_direction: FlexDirection::Column, row_gap: px(4), overflow: Overflow::scroll_y() }
                Children [
                    @feedback(GameplaySidebarFeedbackKind::Mode)--
                    @feedback(GameplaySidebarFeedbackKind::Availability)--
                    @asset_panel()--

                        GameplaySidebarPanel(GameplaySidebarPage::Portal)
                        Node { display: Display::None, width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column, flex_shrink: 0.0 }
                        Children [
                            @crate::ui::context_help::help_button("传送点工具", "当前源点用于查看、改名或删除；目标落点只用于新建传送点，不修改已有连接或自动生成回程。")--
                            @portal_authoring_panel()
                        ] // 双完整列表及名称输入原位保留，不新增第二套作者状态。
                    --
                    @boundary_panel()--
                    @feedback(GameplaySidebarFeedbackKind::Message)
                ]

        ]
    }
}
