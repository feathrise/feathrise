//! 旧式数字输入使用本库主题并复用统一输入组件。
use crate::colors;
use crate::helpers::{numeric_input_palette, text_shadow};
use bevy::prelude::*;
pub fn spawn_numeric_field_with_size(
    parent: &mut ChildSpawnerCommands,
    root: Entity,
    font: Handle<Font>,
    value: impl Into<String>,
    width: f32,
    height: f32,
    font_size: f32,
) {
    feathrise_ui_widgets::spawn_numeric_field_with_palette(
        parent,
        root,
        font,
        value,
        width,
        height,
        font_size,
        numeric_input_palette(),
        colors::TEXT_ACCENT,
        text_shadow(),
    );
}
