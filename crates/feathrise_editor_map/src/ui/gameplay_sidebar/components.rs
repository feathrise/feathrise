//! 常驻工具标签、正文容器、浏览入口和反馈文本的唯一界面身份。

use super::types::{GameplaySidebarBrowse, GameplaySidebarFeedbackKind, GameplaySidebarPage};
use bevy::prelude::*;

#[derive(Component, Clone, Default)]
pub(super) struct GameplaySidebarRoot;

#[derive(Component, Clone, Default)]
pub(super) struct GameplaySidebarScroll;

#[derive(Component, Clone, Copy)]
pub(super) struct GameplaySidebarTab(pub crate::types::EditorAction); // 不挂通用ActionButton，防止其绕过工具页的拖动/IME门。

impl Default for GameplaySidebarTab {
    fn default() -> Self {
        Self(crate::types::EditorAction::ToolPlaceAsset)
    }
}

#[derive(Component, Clone, Copy, Default)]
pub(super) struct GameplaySidebarPanel(pub GameplaySidebarPage);

#[derive(Component, Clone, Copy, Default)]
pub(super) struct GameplaySidebarBrowseButton(pub GameplaySidebarBrowse);

#[derive(Component, Clone, Copy, Default)]
pub(super) struct GameplaySidebarFeedbackText(pub GameplaySidebarFeedbackKind);
