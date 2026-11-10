//! 固定空间笔刷浮层的标记；与资产变换按钮、数值字段查询保持互斥。
use super::types::Action;
use bevy::prelude::*;
#[derive(Component)]
pub(super) struct Controls;
#[derive(Component)]
pub(super) struct SpatialOnly;
#[derive(Component)]
pub(super) struct ActionButton(pub Action);
#[derive(Component)]
pub(super) struct Coordinate(pub usize);
#[derive(Component)]
pub(super) struct Status;
