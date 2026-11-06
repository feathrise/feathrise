//! 工程目录的内部内容容器、固定可见文件行和轻量筛选字段。

use super::types::{ProjectBrowserView, ProjectFileAction};
use bevy::prelude::Component;

#[derive(Component, Clone, Copy, Default)]
pub(crate) struct ProjectBrowserContent(pub(crate) ProjectBrowserView);

#[derive(Component, Clone, Copy, Default)]
pub(super) struct ProjectFileActionButton(pub(super) ProjectFileAction);

#[derive(Component, Clone, Copy, Default)]
pub(super) struct ProjectFileList {
    pub(super) spawned_rows: usize,
}

#[derive(Component, Clone, Copy, Default)]
pub(crate) struct ProjectFileRow {
    pub(crate) slot: usize,
    pub(crate) entry_index: Option<usize>,
    pub(crate) revision: u64,
    pub(crate) viewport_revision: u64,
}

#[derive(Component, Clone, Copy, Default)]
pub(super) struct ProjectFileRowText;

#[derive(Component, Clone, Copy, Default)]
pub(super) struct ProjectFileRowIcon;

#[derive(Component, Clone, Copy, Default)]
pub(super) struct ProjectFileFolderText;

#[derive(Component, Clone, Copy, Default)]
pub(super) struct ProjectFileSummaryText;

#[derive(Component, Clone, Copy, Default)]
pub(super) struct ProjectFileStatusText;

#[derive(Component, Clone, Copy, Default)]
pub(super) struct ProjectFileSelectionText;

#[derive(Component, Clone, Copy, Default)]
pub(super) struct ProjectFileFilter;
