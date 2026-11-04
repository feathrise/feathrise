//! 自有失败恢复验收的同World实体哨兵，不参与正常作者功能。

use bevy::prelude::*;

#[derive(Component)]
pub(super) struct WorldMarker;
