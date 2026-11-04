//! 水域侧栏常驻导航、滚动正文与短反馈节点；不充当水域对象的稳定身份。

use super::resources::WaterSidebarPage;
use bevy::prelude::*;

#[derive(Component, Clone, Default)]
pub(crate) struct WaterSidebarRoot;

#[derive(Component, Clone, Default)]
pub(crate) struct WaterSidebarScroll;

#[derive(Component, Clone, Default)]
pub(crate) struct WaterSidebarTab(pub(crate) WaterSidebarPage);

#[derive(Component, Clone, Default)]
pub(crate) struct WaterSidebarPanel(pub(crate) WaterSidebarPage);

#[derive(Component, Clone, Default)]
pub(crate) struct WaterSidebarFeedbackText(pub(crate) super::types::WaterSidebarFeedbackKind);
