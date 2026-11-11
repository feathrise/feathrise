//! 植被侧栏固定壳和反馈位置标记，不参与场景或预设的稳定身份。

use bevy::prelude::*;

#[derive(Component, Clone, Default)]
pub(crate) struct VegetationSidebarRoot;

#[derive(Component, Clone, Default)]
pub(crate) struct VegetationSidebarFeedbackText(
    pub(crate) super::types::VegetationSidebarFeedbackKind,
);
