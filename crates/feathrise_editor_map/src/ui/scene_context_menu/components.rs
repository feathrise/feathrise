//! 固定菜单、中文名称输入及各场景列表共享的已呈现行快照。

use super::types::{SceneMenuAction, SceneObjectTarget};
use bevy::prelude::*;

#[derive(Component, Default, Clone, PartialEq, Eq)]
pub(crate) struct SceneObjectRow(pub Option<SceneObjectTarget>);

#[derive(Component)]
pub(super) struct Backdrop;
#[derive(Component)]
pub(super) struct Popup;
#[derive(Component)]
pub(super) struct MenuButton(pub SceneMenuAction);
#[derive(Component)]
pub(super) struct RenamePanel;
#[derive(Component)]
pub(super) struct RenameInput;
#[derive(Component)]
pub(super) struct RenameField;
#[derive(Component)]
pub(super) struct RenameConfirm;
#[derive(Component)]
pub(super) struct RenameCancel;
