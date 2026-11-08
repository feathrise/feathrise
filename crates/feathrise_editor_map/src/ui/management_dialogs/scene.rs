//! 水墨铜木管理弹窗的共用尺寸、阅读层级和原业务动作按钮。

use feathrise_editor_ui::InteractionButton;
use feathrise_editor_ui::metrics as ui_metrics;
use super::components::*;
use crate::components::{MapEditorActionButton, MapEditorCustomActionStyle};
use crate::types::EditorAction;
use crate::ui::scene::FONT_PATH;
use crate::ui::theme as colors;
use bevy::prelude::*;
use bevy::text::{FontSourceTemplate, LineBreak};

pub(crate) fn panel_node() -> Node {
    Node {
        width: px(480),
        max_width: percent(100),
        max_height: percent(100),
        min_width: px(0),
        min_height: px(0),
        flex_shrink: 1.0,
        flex_direction: FlexDirection::Column,
        row_gap: px(8),
        padding: UiRect::all(px(12)),
        border: UiRect::all(px(1)),
        border_radius: BorderRadius::all(px(6)),
        ..default()
    }
}

pub(crate) fn body_node() -> Node {
    // 正文只占最多190px，低窗口继续压缩它，绝不能压缩或滚走底部操作。
    Node {
        width: percent(100),
        min_width: px(0),
        min_height: px(0),
        max_height: px(190),
        flex_shrink: 1.0,
        flex_direction: FlexDirection::Column,
        row_gap: px(7),
        padding: UiRect::right(px(3)),
        overflow: Overflow::scroll_y(),
        ..default()
    }
}

pub(crate) fn feedback_node() -> Node {
    // 原错误完整回显但单独限高；错误不会遮住目标、风险说明或确认按钮。
    Node {
        width: percent(100),
        min_width: px(0),
        min_height: px(28),
        max_height: px(64),
        flex_shrink: 0.0,
        padding: UiRect::axes(px(6), px(4)),
        overflow: Overflow::scroll_y(),
        border_radius: BorderRadius::all(px(3)),
        ..default()
    }
}

pub(crate) fn footer_node() -> Node {
    Node {
        width: percent(100),
        min_width: px(0),
        flex_shrink: 0.0,
        flex_wrap: FlexWrap::Wrap,
        justify_content: JustifyContent::FlexEnd,
        column_gap: px(6),
        row_gap: px(4),
        ..default()
    }
}

pub(crate) fn heading(label: &str) -> impl Scene {
    bsn! {
        Text(label) @feathrise_editor_ui::localization::label_scene(label)
        TextLayout::linebreak(LineBreak::AnyCharacter)
        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_BODY) }
        TextColor(colors::TEXT_ACCENT)
        Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }
    }
}

pub(crate) fn caption(label: &str) -> impl Scene {
    bsn! {
        Text(label) @feathrise_editor_ui::localization::label_scene(label)
        TextLayout::linebreak(LineBreak::AnyCharacter)
        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
        TextColor(colors::TEXT_MUTED)
        Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }
    }
}

pub(crate) fn button(label: &str, action: EditorAction) -> impl Scene {
    bsn! {
        InteractionButton ManagementButton MapEditorActionButton(action)
        // 专属样式标记避开通用按钮回显，危险色不会被每帧改回普通铜色。
        MapEditorCustomActionStyle
        Node {
            height: px(26), min_width: px(0), max_width: percent(100), flex_shrink: 0.0,
            align_items: AlignItems::Center, justify_content: JustifyContent::Center,
            padding: UiRect::horizontal(px(9)), border: px(1),
            border_radius: BorderRadius::all(px(3)),
        }
        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL)
        BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [(
            ManagementButtonText
            Text(label) @feathrise_editor_ui::localization::label_scene(label)
            TextLayout::no_wrap()
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
            TextColor(colors::TEXT_PRIMARY)
        )]
    }
}

pub(crate) fn folder_confirm_button() -> impl Scene {
    bsn! {
        InteractionButton ManagementButton MapEditorCustomActionStyle
        MapEditorActionButton(EditorAction::ConfirmMapFolderDialog)
        Node {
            height: px(26), min_width: px(0), max_width: percent(100), flex_shrink: 0.0,
            align_items: AlignItems::Center, justify_content: JustifyContent::Center,
            padding: UiRect::horizontal(px(9)), border: px(1),
            border_radius: BorderRadius::all(px(3)),
        }
        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL)
        BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [(
            FolderConfirmText ManagementButtonText
            Text("确认") @feathrise_editor_ui::localization::label_scene("确认") TextLayout::no_wrap()
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
            TextColor(colors::TEXT_PRIMARY)
        )]
    }
}
