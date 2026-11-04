//! 环境页的只读回显与地图属性容器标记，不携带作者动作。

use bevy::prelude::*;

use super::types::EnvironmentTextField;

#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct EnvironmentSidebarText(pub(super) EnvironmentTextField);

#[derive(Component, Clone, Copy, Default)]
pub(crate) struct EnvironmentMapControls;
