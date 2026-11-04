//! 地图目录浮层的固定遮罩与弹出面板标记。

use bevy::prelude::*;

#[derive(Component, Clone, Default)]
pub(super) struct DirectoryMenuBackdrop;

#[derive(Component, Clone, Default)]
pub(super) struct DirectoryMenuPopup;
