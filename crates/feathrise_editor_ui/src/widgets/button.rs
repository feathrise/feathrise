//! 编辑器按钮样式；交互实体树复用无主题控件。
use crate::{ButtonColors, TextButtonStyle, colors, text_shadow};
use bevy::prelude::*;

/// 所有作者对话框共用紧凑按钮、弱边界和原输入协议；主动作与开关只改变填充。
pub fn dialog_button_style(primary: bool) -> TextButtonStyle {
    TextButtonStyle {
        node: Node {
            height: px(28), min_width: px(64), max_width: percent(100), flex_shrink: 0.,
            padding: UiRect::horizontal(px(10)),
            align_items: AlignItems::Center, justify_content: JustifyContent::Center,
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(crate::metrics::RADIUS)), ..default()
        },
        colors: if primary {
            ButtonColors::new(colors::SELECT_BG, colors::SELECT_BG_HOVER, colors::SELECT_BG_PRESSED)
        } else {
            ButtonColors::new(colors::BUTTON_DEV_SMALL_NORMAL, colors::BUTTON_DEV_SMALL_HOVERED, colors::BUTTON_DEV_SMALL_PRESSED)
        },
        font_size: crate::metrics::FONT_CONTROL,
        text_color: colors::TEXT_PRIMARY,
        border_color: Some(colors::DIALOG_BORDER),
    }
}

/// 兼容已有确认按钮入口，避免关闭工程继续保留单独的样式版本。
pub fn confirmation_button_style(primary: bool) -> TextButtonStyle {
    dialog_button_style(primary)
}

/// BSN共用相同配色组件；ButtonColors没有无主题默认色，不能用要求Default的template_value强行构造。
pub fn dialog_button_colors_scene(primary: bool) -> impl Scene {
    bevy::ecs::template::template(move |_| Ok(dialog_button_style(primary).colors))
}

pub fn dialog_close_button_style() -> TextButtonStyle {
    let mut style = dialog_button_style(false);
    style.node.width = px(28);
    style.node.min_width = px(28);
    style.node.padding = UiRect::ZERO;
    style
}

pub fn dialog_compact_button_style(primary: bool) -> TextButtonStyle {
    let mut style = dialog_button_style(primary);
    style.node.height = px(22);
    style.node.min_width = px(0);
    style.node.padding = UiRect::horizontal(px(4));
    style
}

pub fn dialog_compact_close_button_style() -> TextButtonStyle {
    let mut style = dialog_compact_button_style(false);
    style.node.width = px(22);
    style.node.min_width = px(22);
    style.node.padding = UiRect::ZERO;
    style
}

pub fn menu_button_style(width: f32, height: f32, font_size: f32) -> TextButtonStyle {
    TextButtonStyle {
        node: Node {
            width: Val::Px(width),
            height: Val::Px(height),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(6.0)),
            ..default()
        },
        colors: ButtonColors::new(
            colors::BUTTON_DEV_SMALL_NORMAL,
            colors::BUTTON_DEV_SMALL_HOVERED,
            colors::BUTTON_DEV_SMALL_PRESSED,
        ),
        font_size: crate::metrics::font_size(font_size),
        text_color: colors::TEXT_PRIMARY,
        border_color: Some(colors::COPPER_BORDER_WEAK),
    }
}
pub fn spawn_custom_text_button<C: Component>(
    parent: &mut ChildSpawnerCommands,
    marker: C,
    font: Handle<Font>,
    label: impl Into<String>,
    mut style: TextButtonStyle,
) {
    style.font_size = crate::metrics::font_size(style.font_size);
    feathrise_ui_widgets::spawn_custom_text_button(
        parent,
        marker,
        font,
        label,
        style,
        text_shadow(),
    );
}

/// 动态业务文案保留同一按钮树、字体度量与阴影，不让调用方另造一套对话框按钮。
pub fn spawn_custom_text_button_with_text_marker<C: Component, T: Component>(
    parent: &mut ChildSpawnerCommands,
    marker: C,
    text_marker: T,
    font: Handle<Font>,
    label: impl Into<String>,
    mut style: TextButtonStyle,
) {
    style.font_size = crate::metrics::font_size(style.font_size);
    feathrise_ui_widgets::spawn_custom_text_button_with_text_marker(
        parent, marker, text_marker, font, label, style, text_shadow(),
    );
}
