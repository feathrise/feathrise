//! 植被全局预设库、独立诊断及中央浏览器式工作区静态挂载点。

use feathrise_editor_ui::InteractionButton;
use feathrise_editor_ui::metrics as ui_metrics;
use crate::ui::theme as colors;
use bevy::prelude::*; // BSN Scene、布局与 UI 组件。
use bevy::text::FontSourceTemplate; // 中文名称与诊断使用项目统一字体。
use bevy::ui::RelativeCursorPosition; // 中央详细页按悬停接管滚轮，沿用F12水墨铜木配色。

use super::types::{
    PresetEditorAction, PresetEditorButton, PresetEditorStatusText, PresetEditorSummaryText,
    PresetWorkspaceEditorHost, PresetWorkspaceEditorPage, PresetWorkspaceTabBar, PresetWorkspaceTabHost,
}; // 静态壳只携带动作、回显和动态挂载 marker。
use crate::components::{MapEditorPresetTabScroll, MapEditorPresetWorkspaceScroll}; // 标签横滚与正文纵滚互不抢占。
use crate::ui::scene::FONT_PATH; // 复用已验证中文字体路径。

/// 构造预设管理与标签条共用的紧凑铜边按钮。
fn preset_button(label: &str, action: PresetEditorAction) -> impl Scene {
    bsn! {
        InteractionButton
        PresetEditorButton(action)
        Node {
            height: px(24),
            flex_grow: 1.0,
            flex_basis: px(0),
            min_width: px(0),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(3)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            padding: UiRect::horizontal(px(4)),
        }
        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL)
        BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [
            Text(label) @feathrise_editor_ui::localization::label_scene(label)
            TextFont {
                font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)),
                font_size: px(ui_metrics::FONT_CONTROL),
            }
            TextColor(colors::TEXT_PRIMARY)
        ]
    }
}

/// 构造左栏一行两个等宽动作按钮。
fn two_button_row(
    first_label: &str,
    first_action: PresetEditorAction,
    second_label: &str,
    second_action: PresetEditorAction,
) -> impl Scene {
    bsn! {
        Node {
            width: percent(100),
            column_gap: px(3),
            flex_shrink: 0.0,
        }
        Children [
            @preset_button(first_label, first_action)--
            @preset_button(second_label, second_action)
        ]
    }
}

/// 全局库用普通分区，不嵌套菜单卡片；与地图增删按钮明确区分。
pub(crate) fn vegetation_preset_editor_panel() -> impl Scene {
    bsn! {
        Node {
            width: percent(100),
            min_width: px(0),
            flex_direction: FlexDirection::Column,
            row_gap: px(4),
            flex_shrink: 0.0,
        }
        Children [

                Text("全局预设库") @feathrise_editor_ui::localization::label_scene("全局预设库")
                TextFont {
                    font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)),
                    font_size: px(ui_metrics::FONT_CONTROL),
                }
                TextColor(colors::TEXT_ACCENT)
                Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }
            --

                Text("所有地图共用；编辑或删除预设不等于删除当前地图图层。") @feathrise_editor_ui::localization::label_scene("所有地图共用；编辑或删除预设不等于删除当前地图图层。")
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
                TextColor(colors::TEXT_MUTED)
                TextLayout { linebreak: LineBreak::AnyCharacter }
                Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }
            --
            @two_button_row("新建草本预设", PresetEditorAction::CreateBlade, "新建模型预设", PresetEditorAction::CreateMesh)--
            @two_button_row("编辑所选预设", PresetEditorAction::EditSelected, "删除所选预设", PresetEditorAction::DeleteSelected)-- // 统一所选对象用语。

                PresetEditorSummaryText
                Text("当前预设：尚未选择") @feathrise_editor_ui::localization::label_scene("当前预设：尚未选择")
                TextFont {
                    font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)),
                    font_size: px(ui_metrics::FONT_CONTROL), // 主要选择摘要不再使用过小字号。
                }
                TextColor(colors::TEXT_VALUE)
                TextLayout { linebreak: LineBreak::AnyCharacter }
                Node {
                    width: percent(100),
                    min_width: px(0),
                    flex_shrink: 0.0,
                }

        ]
    }
}

/// 唯一预设诊断移入工具反馈区；仍由原预设域维护，不复制状态或解析错误字符串。
pub(crate) fn vegetation_preset_feedback() -> impl Scene {
    bsn! {
        PresetEditorStatusText
        ~{crate::ui::status_bar::PanelStatusText("植被预设")} feathrise_editor_ui::feedback::GlobalPanelStatus
        Text("") // 操作教程位于作者工具的?帮助，反馈节点只承载原领域产生的真实诊断。
        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
        TextColor(colors::TEXT_MUTED)
        TextLayout { linebreak: LineBreak::AnyCharacter }
        Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }
    }
}

/// 地图与预设文档共用同一横向滚动标签条，空世界不生成伪地图标签。
pub(crate) fn vegetation_workspace_tab_bar() -> impl Scene {
    bsn! {
        PresetWorkspaceTabBar
        crate::workbench::WorkbenchDocumentTabs
        Node {
            width: percent(100),
            height: px(28),
            flex_shrink: 0.0,
            align_items: AlignItems::Center,
            column_gap: px(3),
            padding: UiRect::horizontal(px(4)), // 内高完整保留28px，与真实标签同高，避免上下裁切。
            overflow: Overflow::clip_x(),
        }
        BackgroundColor(colors::PANEL_BG_STRONG)
        BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [

                MapEditorPresetTabScroll
                PresetWorkspaceTabHost
                RelativeCursorPosition::default()
                ScrollPosition::default()
                Node {
                    height: percent(100),
                    flex_grow: 1.0,
                    min_width: px(0),
                    align_items: AlignItems::Center,
                    column_gap: px(3),
                    overflow: Overflow::scroll_x(),
                }

        ]
    }
}

/// 返回中央详细编辑页；活动草稿内容由 presentation 系统动态生成。
pub(crate) fn vegetation_workspace_editor_page() -> impl Scene {
    bsn! {
        PresetWorkspaceEditorPage
        MapEditorPresetWorkspaceScroll
        RelativeCursorPosition::default()
        ScrollPosition::default()
        Node {
            display: Display::None,
            width: percent(100),
            flex_grow: 1.0,
            min_height: px(0),
            flex_direction: FlexDirection::Column,
            overflow: Overflow::scroll_y(),
            padding: UiRect::all(px(5)),
        }
        BackgroundColor(colors::MODEL_PREVIEW_BG)
        Children [

            PresetWorkspaceEditorHost
            Node {
                width: percent(100),
                flex_direction: FlexDirection::Column,
                row_gap: px(5),
                flex_shrink: 0.0,
            }
        ] // 中央只保留作者表单；各类样品和唯一Asset预览位于右侧场景页。
    }
}
