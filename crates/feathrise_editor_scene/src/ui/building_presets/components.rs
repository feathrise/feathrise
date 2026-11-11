//! 建筑工作台的文档层级与按钮视觉标记；不进入目录、场景或存档。

use bevy::prelude::*;

use super::types::BuildingPresetAction;

/// 工作台按钮独立于左栏列表着色，避免列表同步覆盖主保存、导航和输入槽。
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BuildingButtonKind {
    Ordinary,
    Primary,
    Navigation,
    Field,
}

#[derive(Component)]
pub(super) struct BuildingDocumentHeader;

#[derive(Component)]
pub(super) struct BuildingDocumentBadge;

#[derive(Component)]
pub(super) struct BuildingDocumentNavigation;

#[derive(Component)]
pub(super) struct BuildingDocumentForm;

/// 侧栏参数标签仍绑定原动作；回显当前值而不建立第二套控件或作者参数。
#[derive(Component, Clone, Copy, Debug)]
pub(super) struct BuildingPlacementOptionText(pub(super) BuildingPresetAction);

impl Default for BuildingPlacementOptionText {
    fn default() -> Self {
        Self(BuildingPresetAction::CycleGrid)
    }
}

/// 固定侧栏按钮独立刷新参数高亮，不让一次开关变化遍历整个动态预设目录。
#[derive(Component, Clone, Default)]
pub(super) struct BuildingSidebarActionButton;
