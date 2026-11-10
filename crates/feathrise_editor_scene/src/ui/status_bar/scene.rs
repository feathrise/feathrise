//! 24像素固定底栏与根直属非模态详情；展开只覆盖画面，不参与三栏布局。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::InteractionButton;
use feathrise_editor_ui::metrics as ui_metrics;
use bevy::prelude::*;
use bevy::text::{FontSourceTemplate, LineBreak, LineHeight};
use bevy::ui::{FocusPolicy, RelativeCursorPosition};

use super::components::*;
use super::types::{StatusAction, StatusTextRole};
use crate::components::{SceneEditorCurrentSceneText, SceneEditorStatusText};
use crate::ui::scene::FONT_PATH;
use crate::ui::theme as colors;

fn text(role: StatusTextRole, value: &'static str) -> impl Scene {
    bsn! {
        StatusText(role)
        Text(value)
        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
        TextColor(colors::TEXT_PRIMARY)
        TextLayout::no_wrap()
        ~{FocusPolicy::Pass}
        Node { min_width: px(0), overflow: Overflow::clip_x() }
    }
}

fn button(action: StatusAction, label: &'static str, height: f32) -> impl Scene {
    bsn! {
        InteractionButton
        StatusButton(action)
        Node { height: px(height), min_width: px(0), flex_shrink: 0.0,
            align_items: AlignItems::Center, justify_content: JustifyContent::Center,
            padding: UiRect::horizontal(px(4)), border_radius: BorderRadius::all(px(3)) }
        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL)
        Children [
            Text(label) @feathrise_editor_ui::localization::label_scene(label)
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
            TextColor(colors::TEXT_PRIMARY)
            TextLayout::no_wrap()
            ~{FocusPolicy::Pass}
        ]
    }
}

pub(in crate::ui) fn footer() -> impl Scene {
    bsn! {
        StatusFooter
        // 父工作台为420；Bevy 0.20把全局层级子树独立排序，同值可能让父背景盖住整行。
        GlobalZIndex(421) // 常态明确高于父背景；加载期间由同步系统提升到输入门之上。
        Node { width: percent(100), height: px(24), min_width: px(0), flex_shrink: 0.0,
            align_items: AlignItems::Center, column_gap: px(6), padding: UiRect::horizontal(px(8)),
            border: UiRect::top(px(1)), overflow: Overflow::clip_x() }
        BackgroundColor(colors::PANEL_BG_STRONG)
        BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [
            StatusFooterContent
                Node { flex_grow: 1.0, flex_shrink: 1.0, min_width: px(0),
                    align_items: AlignItems::Center, column_gap: px(6), overflow: Overflow::clip_x() }
                Children [
                    Node { min_width: px(38), flex_shrink: 0.0 } Children [@text(StatusTextRole::Phase, "就绪")]--
                    // 裁切必须由文字的父节点承担，避免多段英文跨入邻格。详情仍展示完整回执。
                    Node { flex_grow: 1.0, flex_basis: px(0), min_width: px(0), overflow: Overflow::clip_x() }
                        Children [
                            SceneEditorStatusText StatusText(StatusTextRole::Summary)
                            Text("编辑器正在初始化") @feathrise_editor_ui::localization::label_scene("编辑器正在初始化")
                            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                            TextColor(colors::TEXT_PRIMARY) TextLayout::no_wrap()
                            ~{FocusPolicy::Pass}
                            Node { min_width: px(0) }
                        ]--
                    Node { max_width: percent(30), min_width: px(0), overflow: Overflow::clip_x() }
                        Children [@text(StatusTextRole::Task, "")]--
                    Node { width: percent(36), max_width: px(420), min_width: px(0), flex_shrink: 1.0,
                        justify_content: JustifyContent::FlexEnd, overflow: Overflow::clip_x() }
                        Children [
                            SceneEditorCurrentSceneText
                            Text("当前场景：等待目录") @feathrise_editor_ui::localization::label_scene("当前场景：等待目录")
                            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                            TextColor(colors::TEXT_MUTED) TextLayout::no_wrap()
                            ~{FocusPolicy::Pass}
                            Node { max_width: percent(100), min_width: px(0), flex_shrink: 1.0, overflow: Overflow::clip_x() }
                        ]-- // 场景信息位于视图摘要左侧；长名称只裁切，最右保留详情入口。
                    Node { max_width: percent(25), min_width: px(0), overflow: Overflow::clip_x() }
                        Children [@text(StatusTextRole::Context, "")]
                ]--
            @super::super::scene_loading::scene_loading_panel()--
            // 切换场景仅替换左侧内容，不卸载或挤走最右的详情入口。
            @button(StatusAction::Toggle, "详情", 20.0)--
            @log_button()
        ]
    }
}

fn log_button() -> impl Scene {
    bsn! {
        InteractionButton
        crate::workbench::WorkbenchProjectLogButton
        Node { display: Display::None, height: px(20), flex_shrink: 0.0,
            align_items: AlignItems::Center, justify_content: JustifyContent::Center,
            padding: UiRect::horizontal(px(4)), border_radius: BorderRadius::all(px(3)) }
        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL)
        Children [
            Text("日志") @feathrise_editor_ui::localization::label_scene("日志")
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
            TextColor(colors::TEXT_PRIMARY) TextLayout::no_wrap()
            ~{FocusPolicy::Pass}
        ]
    }
}

pub(in crate::ui) fn popup() -> impl Scene {
    bsn! {
        StatusPopup
        RelativeCursorPosition::default()
        ~{FocusPolicy::Block}
        UiInteraction::default()
        // 左右锚点由实际根宽约束；低高度只缩小正文，顶栏与固定动作不滚动。
        ~{Node { display: Display::None, position_type: PositionType::Absolute,
            left: px(8), right: px(8), bottom: px(32), max_width: px(760), height: px(280),
            max_height: percent(60), ..feathrise_editor_ui::dialog_panel_node() }}
        GlobalZIndex(425) // 位于工作区上方、管理模态下方；不是新的全屏遮罩。
        BackgroundColor(feathrise_editor_ui::colors::DIALOG_BG)
        BorderColor::all(feathrise_editor_ui::colors::DIALOG_BORDER)
        Children [
            StatusPopupHeader ~{feathrise_editor_ui::dialog_header_node()}
                BackgroundColor(feathrise_editor_ui::colors::DIALOG_HEADER_BG)
                BorderColor::all(feathrise_editor_ui::colors::DIALOG_BORDER)
                Children [
                    StatusText(StatusTextRole::Title) Text("状态详情") @feathrise_editor_ui::localization::label_scene("状态详情")
                        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_BODY) }
                        TextColor(colors::TEXT_PRIMARY) TextLayout::no_wrap()
                        ~{FocusPolicy::Pass}
                        Node { flex_grow: 1.0, min_width: px(0), overflow: Overflow::clip_x() }--
                    @popup_button(StatusAction::Close, "×", true)
                ]--
            StatusDetailScroll RelativeCursorPosition::default() ScrollPosition::default()
                ~{FocusPolicy::Block}
                ~{detail_scroll_node(false)}
                Children [
                    StatusText(StatusTextRole::Detail) Text("")
                    TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                    ~{LineHeight::Px(16.0)}
                    TextColor(colors::TEXT_PRIMARY) TextLayout::linebreak(LineBreak::WordOrCharacter)
                    ~{FocusPolicy::Pass}
                    Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }
                ]--
            StatusPopupFooter ~{feathrise_editor_ui::dialog_footer_node()}
                BackgroundColor(feathrise_editor_ui::colors::DIALOG_HEADER_BG)
                BorderColor::all(feathrise_editor_ui::colors::DIALOG_BORDER)
                Children [
                    // 复制结果保留在操作区，不再替换窗口标题或改变诊断正文身份。
                    StatusText(StatusTextRole::CopyResult) Text("")
                        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
                        TextColor(colors::TEXT_MUTED) TextLayout::no_wrap()
                        ~{FocusPolicy::Pass}
                        Node { display: Display::None, flex_grow: 1.0, flex_basis: px(0), min_width: px(0), overflow: Overflow::clip_x() }--
                    @popup_button(StatusAction::Copy, "复制", false)--
                    @popup_button(StatusAction::Dismiss, "清除", false)
                ]
        ]
    }
}

pub(super) fn detail_scroll_node(compact: bool) -> Node {
    Node {
        flex_grow: 1.0, flex_shrink: 1.0, overflow: Overflow::scroll_y(),
        ..if compact { feathrise_editor_ui::dialog_compact_body_node() }
          else { feathrise_editor_ui::dialog_body_node() }
    }
}

fn popup_button(action: StatusAction, label: &'static str, close: bool) -> impl Scene {
    let style = if close { feathrise_editor_ui::dialog_close_button_style() }
        else { feathrise_editor_ui::dialog_button_style(false) };
    bsn! {
        InteractionButton StatusButton(action) ~{style.node} @feathrise_editor_ui::dialog_button_colors_scene(false)
        ~{BackgroundColor(style.colors.normal)} BorderColor::all(feathrise_editor_ui::colors::DIALOG_BORDER)
        Children [
            Text(label) @feathrise_editor_ui::localization::label_scene(label) TextLayout::no_wrap()
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
            TextColor(colors::TEXT_PRIMARY) ~{FocusPolicy::Pass}
        ]
    }
}
