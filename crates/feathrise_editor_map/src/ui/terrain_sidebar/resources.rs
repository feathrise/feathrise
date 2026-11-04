//! 地形侧栏只记忆工具页和最后一种雕刻工具，不进入地图、设置或撤销历史。

use bevy::prelude::*;

use crate::types::{EditorAction, EditorTool};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum TerrainSidebarPage {
    #[default]
    Sculpt,
    Surface,
    Puddles,
}

impl TerrainSidebarPage {
    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Sculpt => "雕刻",
            Self::Surface => "地表",
            Self::Puddles => "积水",
        }
    }

    pub(super) const fn for_tool(tool: EditorTool) -> Option<Self> {
        match tool {
            EditorTool::Raise
            | EditorTool::Lower
            | EditorTool::Smooth
            | EditorTool::Flatten
            | EditorTool::VolumeDig
            | EditorTool::VolumeFill
            | EditorTool::VolumeSmooth
            | EditorTool::VolumeFlatten => Some(Self::Sculpt),
            EditorTool::Surface => Some(Self::Surface),
            EditorTool::Puddles => Some(Self::Puddles),
            _ => None,
        }
    }
}

#[derive(Resource)]
pub(super) struct TerrainSidebarState {
    pub(super) page: TerrainSidebarPage,
    pub(super) sculpt_tool: EditorTool,
}

impl Default for TerrainSidebarState {
    fn default() -> Self {
        Self {
            page: TerrainSidebarPage::Sculpt,
            sculpt_tool: EditorTool::Raise,
        }
    }
}

impl TerrainSidebarState {
    pub(super) fn observe_tool(&mut self, tool: EditorTool) {
        if let Some(page) = TerrainSidebarPage::for_tool(tool) {
            self.page = page;
            if page == TerrainSidebarPage::Sculpt {
                self.sculpt_tool = tool;
            }
        }
    }

    pub(super) const fn requested_tool(&self) -> EditorTool {
        match self.page {
            TerrainSidebarPage::Sculpt => self.sculpt_tool,
            TerrainSidebarPage::Surface => EditorTool::Surface,
            TerrainSidebarPage::Puddles => EditorTool::Puddles,
        }
    }

    pub(super) const fn requested_action(&self) -> EditorAction {
        match self.requested_tool() {
            EditorTool::Raise => EditorAction::ToolRaise,
            EditorTool::Lower => EditorAction::ToolLower,
            EditorTool::Smooth => EditorAction::ToolSmooth,
            EditorTool::Flatten => EditorAction::ToolFlatten,
            EditorTool::VolumeDig => EditorAction::ToolVolumeDig,
            EditorTool::VolumeFill => EditorAction::ToolVolumeFill,
            EditorTool::VolumeSmooth => EditorAction::ToolVolumeSmooth,
            EditorTool::VolumeFlatten => EditorAction::ToolVolumeFlatten,
            EditorTool::Surface => EditorAction::ToolSurface,
            _ => EditorAction::ToolPuddles,
        }
    }
}
