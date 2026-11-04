//! 固定管理弹窗的布局和回显标记；业务目标仍来自原冻结请求。

use super::DialogKind;
use bevy::prelude::*;

#[derive(Component, Clone, Copy, Default)]
pub(crate) struct ManagementPanel(pub(crate) DialogKind);

#[derive(Component, Clone, Copy, Default)]
pub(crate) struct ManagementScroll(pub(crate) DialogKind);

#[derive(Component, Clone, Copy, Default)]
pub(crate) struct ManagementFeedback(pub(crate) DialogKind);

#[derive(Component, Clone, Copy, Default)]
pub(crate) struct ManagementFooter(pub(crate) DialogKind);

#[derive(Component, Clone, Copy, Default)]
pub(crate) struct ManagementButton;

#[derive(Component, Clone, Copy, Default)]
pub(crate) struct ManagementButtonText;

#[derive(Component, Clone, Copy, Default)]
pub(crate) struct FolderTargetText;

#[derive(Component, Clone, Copy, Default)]
pub(crate) struct FolderConsequencesText;

#[derive(Component, Clone, Copy, Default)]
pub(crate) struct FolderConfirmText;

#[derive(Component, Clone, Copy, Default)]
pub(crate) struct TerrainConflictFeedbackText;
