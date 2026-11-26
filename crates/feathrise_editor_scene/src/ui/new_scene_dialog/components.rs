//! 场景资料模态只保留反馈滚动与固定操作区的表现标记。

use bevy::prelude::*;

#[derive(Component, Clone, Copy, Default)]
pub(crate) struct DialogFeedbackScroll;

#[derive(Component, Clone, Copy, Default)]
pub(super) struct DialogFooter;

/// 复制透过原名称输入框提交，只有创建和资料编辑能写备注。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneNotesFields;

#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneDialogPrimaryAction;

#[derive(Component, Clone, Copy, Default)]
pub(crate) struct CopySavedVersionAction;
