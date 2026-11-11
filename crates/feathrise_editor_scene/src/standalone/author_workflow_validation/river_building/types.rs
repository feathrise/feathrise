//! 原水网/建筑验收的有限状态与小型视口恢复快照；不持有作者文档所有权。

use crate::types::EditorView;
use bevy::prelude::Vec3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(super) enum Phase {
    #[default]
    Idle,
    Focus,
    WaitFocus,
    RequestRiver,
    WaitRiver,
    HoverRiver,
    PressRiver,
    WaitPoint,
    WaitRiverUndo,
    WaitRiverRedo,
    HoverBuilding,
    PressBuilding,
    WaitBuilding,
    WaitBuildingUndo,
    WaitBuildingRedo,
    Complete,
    Failed,
}

pub(super) type View = (EditorView, bool, Vec3, f32, f32, f32);

/// 三个非共线屏幕点产生真实曲线；偏离既有雕刻/表面/Blade和建筑的验收区域。
pub(super) const RIVER_POINTERS: [[f32; 2]; 3] = [[-0.24, -0.18], [0., 0.18], [0.24, -0.18]];
