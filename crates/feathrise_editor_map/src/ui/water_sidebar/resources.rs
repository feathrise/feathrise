//! 水域工具页仅记忆会话内的准备类别，不保存到地图、预设、设置或撤销历史。

use crate::types::EditorTool;
use bevy::prelude::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum WaterSidebarPage {
    #[default]
    Still,
    River,
    Waterfall,
}

impl WaterSidebarPage {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Still => "静水",
            Self::River => "河流",
            Self::Waterfall => "瀑布",
        }
    }

    pub(crate) const fn tool(self) -> EditorTool {
        match self {
            Self::Still => EditorTool::Water,
            Self::River => EditorTool::RiverSpline,
            Self::Waterfall => EditorTool::Waterfall,
        }
    }

    pub(crate) const fn for_tool(tool: EditorTool) -> Option<Self> {
        match tool {
            EditorTool::Water => Some(Self::Still),
            EditorTool::RiverSpline => Some(Self::River),
            EditorTool::Waterfall => Some(Self::Waterfall),
            _ => None,
        }
    }
}

#[derive(Resource, Default)]
pub(super) struct WaterSidebarState {
    pub(super) page: WaterSidebarPage,
}
