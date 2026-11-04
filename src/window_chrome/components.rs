//! 顶栏呈现与窗口动作标记；原业务按钮仍保留原来的组件、实体和点击协议。
use super::types::{Glyph, Menu, WindowAction};
use bevy::prelude::*;

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ChromeSlot {
    Modes,
    Profile,
    Project,
}
#[derive(Component)]
pub(super) struct Mounted;
#[derive(Component, Clone, Copy)]
pub(super) enum ChromeButton {
    Window(WindowAction),
    Toggle(Menu),
}
#[derive(Component)]
pub(super) struct Popup(pub Menu);
#[derive(Component)]
pub(super) struct Backdrop;
#[derive(Component)]
pub(super) struct Icon(pub Glyph);
#[derive(Component)]
pub(super) struct IconStroke;
