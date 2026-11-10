//! 两种三维视口的用户选择；只决定呈现与预览路由，不进入工程或场景文件格式。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ViewportMode {
    /// 保留原作者 World 与保存后新建游戏 World 的比较基线。
    #[default]
    SavedWorld,
    /// 游戏拥有中央三维画面，并在编辑和预览之间保留同一 World。
    EmbeddedWorld,
}

impl ViewportMode {
    /// 工程3D依赖Win32所属子窗口；Unix保留编辑器 3D 和工程独立启动入口。
    pub(crate) const fn supported(self) -> bool {
        matches!(self, Self::SavedWorld) || cfg!(windows)
    }

    pub(crate) fn available() -> &'static [Self] {
        if cfg!(windows) {
            &[Self::SavedWorld, Self::EmbeddedWorld]
        } else {
            &[Self::SavedWorld]
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::SavedWorld => "编辑器 3D",
            Self::EmbeddedWorld => "工程 3D",
        }
    }
}
