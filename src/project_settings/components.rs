//! 工程设置页、标签和独立输入的 ECS 标记。

use bevy::prelude::*;

#[derive(Component)]
pub(super) struct SettingsTab;
#[derive(Component)]
pub(super) struct SettingsBody;
#[derive(Component)]
pub(super) struct SettingsStatus;
#[derive(Component)]
pub(super) struct SettingsIcon;
#[derive(Component)]
pub(super) struct SettingsIconPath;

#[derive(Component, Clone, Copy)]
pub(super) enum Field {
    Name,
    Description,
}

#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(super) enum Action {
    Activate,
    Close,
    ChooseIcon,
    ResetIcon,
    Save,
    Reload,
}
