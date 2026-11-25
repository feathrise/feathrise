//! 常驻工具标签、正文容器、浏览入口和反馈文本的唯一界面身份。

use super::types::{GameplaySidebarBrowse, GameplaySidebarFeedbackKind, GameplaySidebarPage};
use bevy::prelude::*;

#[derive(Component, Clone, Default)]
pub(super) struct GameplaySidebarRoot;

#[derive(Component, Clone, Default)]
pub(super) struct GameplaySidebarScroll;

/// 页签直接绑定本页身份，统一由玩法工具的输入门受理。
#[derive(Component, Clone, Copy, Default)]
pub(super) struct GameplaySidebarTab(pub GameplaySidebarPage);

#[derive(Component, Clone, Copy, Default)]
pub(super) struct GameplaySidebarPanel(pub GameplaySidebarPage);

#[derive(Component, Clone, Copy, Default)]
pub(super) struct GameplaySidebarBrowseButton(pub GameplaySidebarBrowse);

#[derive(Component, Clone, Copy, Default)]
pub(super) struct GameplaySidebarFeedbackText(pub GameplaySidebarFeedbackKind);
