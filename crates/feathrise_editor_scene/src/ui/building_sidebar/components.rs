//! 固定工具页按钮和正文/反馈挂载标记，不参与原有领域动作分派。

use bevy::prelude::*;

use super::resources::BuildingSidebarPage;

#[derive(Component, Clone, Default)]
pub(super) struct BuildingSidebarRoot;

#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct BuildingSidebarTab(pub(super) BuildingSidebarPage);

#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct BuildingSidebarPanel(pub(super) BuildingSidebarPage);
