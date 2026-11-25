//! 工具页、浏览入口与反馈位置的固定显示契约，不包含作者对象或撤销数据。

use crate::types::EditorTool;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum GameplaySidebarPage {
    #[default]
    Asset,
    Portal,
    Boundary,
    MotionTrigger,
}

impl GameplaySidebarPage {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Asset => "资产摆放",
            Self::Portal => "传送点",
            Self::Boundary => "空气墙",
            Self::MotionTrigger => "动画触发",
        }
    }

    pub(crate) const fn tool(self) -> EditorTool {
        match self {
            Self::Asset => EditorTool::PlaceAsset,
            Self::Portal => EditorTool::Portal,
            Self::Boundary => EditorTool::Boundary,
            Self::MotionTrigger => EditorTool::MotionTrigger,
        }
    }

    pub(crate) const fn for_tool(tool: EditorTool) -> Option<Self> {
        match tool {
            EditorTool::PlaceAsset => Some(Self::Asset),
            EditorTool::Portal => Some(Self::Portal),
            EditorTool::Boundary => Some(Self::Boundary),
            EditorTool::MotionTrigger => Some(Self::MotionTrigger),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) enum GameplaySidebarBrowse {
    #[default]
    Assets,
    Scene,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum GameplaySidebarFeedbackKind {
    #[default]
    Mode,
    Availability,
    Message,
    Asset,
}
