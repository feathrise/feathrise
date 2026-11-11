//! 工程入口的固定按钮与弹出面板标记；不复用场景作者动作ID。

use bevy::prelude::*;

#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(super) enum ProjectAction {
    Toggle,
    SetStartup,
    Export,
    Plugins,
    BackToManager,
}

#[derive(Component)]
pub(super) struct ProjectMenuOverlay;
#[derive(Component)]
pub(super) struct ProjectMenuPanel;
#[derive(Component)]
pub(super) struct ProjectMenuStatus;
