//! 常驻工具标签、正文容器、浏览入口和反馈文本的唯一界面身份。

use super::types::{GameplaySidebarFeedbackKind, GameplaySidebarPage};
use bevy::prelude::*;

#[derive(Component, Clone, Default)]
pub(super) struct GameplaySidebarRoot;

/// 工程规则表单在右栏的宿主，三个工具共用同一个文档草稿。
#[derive(Component, Clone, Default)]
pub(crate) struct InteractionRulesHost;

#[derive(Component, Clone, Default)]
pub(super) struct ViewportFeedback;

#[derive(Component, Clone, Default)]
pub(super) struct GameplaySidebarScroll;

/// 页签直接绑定本页身份，统一由玩法工具的输入门受理。
#[derive(Component, Clone, Copy, Default)]
pub(super) struct GameplaySidebarTab(pub GameplaySidebarPage);

#[derive(Component, Clone, Copy, Default)]
pub(super) struct GameplaySidebarPanel(pub GameplaySidebarPage);

#[derive(Component, Clone, Copy, Default)]
pub(super) struct GameplaySidebarFeedbackText(pub GameplaySidebarFeedbackKind);
