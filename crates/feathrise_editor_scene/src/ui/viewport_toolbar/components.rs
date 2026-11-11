//! 场景工具条的分区布局标记；不记录场景、数值或视图的第二份状态。

use bevy::prelude::*;

#[derive(Component, Clone, Copy, Default)]
pub(in crate::ui) struct ViewportToolbar;

#[derive(Component, Clone, Copy, Default)]
pub(in crate::ui) struct ViewportViewControls;

#[derive(Component, Clone, Copy, Default)]
pub(crate) struct ViewportBrushParameters; // 笔刷同步系统签名需要此可见性，仅控制同排参数显隐。

#[derive(Component, Clone, Copy, Default)]
pub(crate) struct ViewportToolbarScroll; // 地图和 GLB 都只用这一个横滚内容区。

#[derive(Component, Clone, Copy, Default)]
pub(in crate::ui) struct ViewportToolbarExpandButton;
