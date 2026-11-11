//! 植被工作台的文档层级和控件视觉身份；不承载预设、场景或运行时数据。

use bevy::prelude::*;

use super::types::PresetField;

#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum PresetButtonKind {
    #[default]
    Ordinary,
    Primary,
    Navigation,
}

#[derive(Component)]
pub(super) struct PresetDocumentHeader;

#[derive(Component)]
pub(super) struct PresetDocumentBadge;

#[derive(Component)]
pub(super) struct PresetEditorNavigation;

#[derive(Component)]
pub(super) struct PresetEditorForm;

/// 保留官方单行编辑器，另以可换行文本回显完整路径，避免非焦点时只能看见前缀。
#[derive(Component)]
pub(super) struct PresetPathValue(pub PresetField);
