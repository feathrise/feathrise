//! 仅在本次运行记忆准备工具；全局选择/编辑模式仍由视口统一管理。

use super::types::GameplaySidebarPage;
use bevy::prelude::*;

#[derive(Resource, Default)]
pub(crate) struct GameplaySidebarState {
    pub(crate) page: GameplaySidebarPage,
}
