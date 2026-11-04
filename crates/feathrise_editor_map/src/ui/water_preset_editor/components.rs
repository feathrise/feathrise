//! 水体工作台的文档层级与按钮视觉身份；不承载作者数据。

use bevy::prelude::*;

#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum WaterButtonKind {
    #[default]
    Ordinary,
    Primary,
    Navigation,
}

#[derive(Component)]
pub(super) struct WaterDocumentHeader;

#[derive(Component)]
pub(super) struct WaterDocumentBadge;

#[derive(Component)]
pub(super) struct WaterEditorNavigation;

#[derive(Component)]
pub(super) struct WaterEditorForm;
