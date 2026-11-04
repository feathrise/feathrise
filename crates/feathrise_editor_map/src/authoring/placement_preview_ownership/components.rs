//! 正式摆放根被独立作者预览遮蔽时的精确归还责任。

use bevy::prelude::*;

/// 保存前后并存的正式根只能由当前预览拥有者归还，旧清理不能解开后来预览的遮蔽。
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct PlacementPreviewSuppression {
    pub(crate) owner: Entity,
    pub(crate) original_visibility: Visibility, // 与场景树的临时隐藏共用底层显隐，不能冻结覆盖后的Hidden。
}
