//! 地图资料模态只保留反馈滚动与固定操作区的表现标记。

use bevy::prelude::*;

#[derive(Component, Clone, Copy, Default)]
pub(crate) struct DialogFeedbackScroll;

#[derive(Component, Clone, Copy, Default)]
pub(super) struct DialogFooter;
