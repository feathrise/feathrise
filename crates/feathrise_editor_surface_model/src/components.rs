//! 地表工作台的文档层级与按钮视觉身份；不承载作者数据。

use bevy::prelude::*;

#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SurfaceButtonKind {
    #[default]
    Ordinary,
    Primary,
    Navigation,
}

#[derive(Component)]
pub struct SurfaceDocumentHeader;

#[derive(Component)]
pub struct SurfaceDocumentBadge;

#[derive(Component)]
pub struct SurfaceEditorNavigation;

#[derive(Component)]
pub struct SurfaceEditorForm;

use super::types::*;

#[derive(Component, Clone, Copy, Debug)]
pub struct SurfacePresetButton(pub SurfacePresetAction);

impl Default for SurfacePresetButton {
    fn default() -> Self {
        Self(SurfacePresetAction::Create)
    }
}

#[derive(Component, Clone, Copy, Debug, Default)]
pub struct SurfacePresetList;

#[derive(Component, Clone, Copy, Debug, Default)]
pub struct SurfacePresetListTitle;

#[derive(Component, Clone, Copy, Debug, Default)]
pub struct SurfacePresetDynamicListRoot;

#[derive(Component, Clone, Copy, Debug, Default)]
pub struct SurfacePresetSummaryText;

#[derive(Component, Clone, Copy, Debug, Default)]
pub struct SurfacePresetStatusText;
