//! 编辑期幽灵标记，不参与正式场景拾取或持久化。

use bevy::prelude::*;

#[derive(Component)]
pub(super) struct CuboidGhost;
