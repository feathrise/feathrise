//! 工具页与反馈位置的固定显示契约，不包含作者对象或撤销数据。

use crate::types::EditorTool;
use crate::ui::asset_preview::documents::ProjectRulesSection;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum GameplaySidebarPage {
    #[default]
    Portal,
    Boundary,
    MotionTrigger,
    Locations,
    Variables,
    Rules,
}

impl GameplaySidebarPage {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Portal => "传送点",
            Self::Boundary => "空气墙",
            Self::MotionTrigger => "动画触发",
            Self::Locations => "地点区域",
            Self::Variables => "工程变量",
            Self::Rules => "触发规则",
        }
    }

    pub(crate) const fn tool(self) -> Option<EditorTool> {
        match self {
            Self::Portal => Some(EditorTool::Portal),
            Self::Boundary => Some(EditorTool::Boundary),
            Self::MotionTrigger => Some(EditorTool::MotionTrigger),
            Self::Locations | Self::Variables | Self::Rules => None,
        }
    }

    pub(crate) const fn section(self) -> Option<ProjectRulesSection> {
        match self {
            Self::Locations => Some(ProjectRulesSection::Locations),
            Self::Variables => Some(ProjectRulesSection::Variables),
            Self::Rules => Some(ProjectRulesSection::Rules),
            _ => None,
        }
    }

    pub(crate) const fn for_tool(tool: EditorTool) -> Option<Self> {
        match tool {
            EditorTool::Portal => Some(Self::Portal),
            EditorTool::Boundary => Some(Self::Boundary),
            EditorTool::MotionTrigger => Some(Self::MotionTrigger),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum GameplaySidebarFeedbackKind {
    #[default]
    Mode,
    Availability,
    Message,
}
