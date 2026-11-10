//! 侧栏当前工具页只属于编辑器会话，不进入作者脏标记、设置或持久化文件。

use bevy::prelude::*;

#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum BuildingSidebarPage {
    #[default]
    Single,
    Courtyard,
}

impl BuildingSidebarPage {
    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Single => "单栋建筑",
            Self::Courtyard => "院落组合",
        }
    }
}
