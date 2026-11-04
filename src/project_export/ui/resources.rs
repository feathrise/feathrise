//! 弹窗输入所有权与系统目录选择任务；不持有工程构建或发布权限。

use bevy::{ecs::message::MessageCursor, input::mouse::MouseWheel, prelude::*, tasks::Task};

#[derive(Resource, Default)]
pub(crate) struct ExportOverlayState {
    pub was_open: bool,
    pub captured: bool,
    pub close_when_idle: bool,
    pub last_destination: String,
    pub open_directory: Option<Task<Result<(), String>>>,
    pub(super) wheel_cursor: MessageCursor<MouseWheel>,
}

impl ExportOverlayState {
    pub(crate) fn captures_input(&self) -> bool { self.captured }

    pub(super) fn capture(&mut self, open: bool, picker: bool) {
        self.captured = open || self.was_open || picker;
        self.was_open = open;
    }
}

#[derive(Default)]
pub(super) struct ExportFolderPicker(pub Option<crate::folder_picker::FolderPicker>);
