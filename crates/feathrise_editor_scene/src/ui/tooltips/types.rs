//! 提示沿用按钮的原动作身份；文案与键位来源不参与业务分派。

use crate::types::{EditorAction, EditorLayoutAction};
use feathrise_editor_core::EditorShortcutAction;

/// 原生child留孔必须读取提示最终定位后的同帧物理边界。
#[derive(bevy::prelude::SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct TooltipPositioned;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum TooltipAction {
    Editor(EditorAction),
    Layout(EditorLayoutAction),
    #[default]
    Settings,
    TerrainParticipation,
    TerrainVisibility,
    TerrainLock,
    ViewportMode(crate::viewport::modes::ViewportMode),
    WeightSelect,
    WeightEdit,
    ViewportOptions,
    AssetThumbnails,
    AssetSearch,
    AssetMissingCheck,
    AssetFavorites,
    AssetRecent,
    ViewShading(crate::viewport::shading::types::ShadingMode),
    OrientationAxis,
    OrientationProjection,
}

pub(super) struct TooltipCopy {
    pub(super) title: &'static str,
    pub(super) description: &'static str,
    pub(super) shortcut: Option<EditorShortcutAction>,
    pub(super) fixed_key: Option<&'static str>,
}
