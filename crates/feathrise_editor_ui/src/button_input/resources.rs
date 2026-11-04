//! 全窗口共用一个左键捕获槽和一个上帧脉冲，不累计按钮或作者数据。

use bevy::prelude::*;

#[derive(Clone, Copy)]
pub(super) struct CapturedButton {
    pub entity: Entity,
    pub parent: Option<Entity>,
    pub camera: Option<Entity>,
    pub generation: u64,
}

#[derive(Resource, Default)]
pub(super) struct ButtonCapture {
    pub held: Option<CapturedButton>,
    pub released: Option<Entity>,
}
