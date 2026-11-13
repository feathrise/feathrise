//! 编辑器桌面内的文件管理；固定文件栏与多窗口共享真实后台操作及剪贴板。

pub(crate) mod browser;
mod components;
mod context_menu;
mod input;
mod messages;
mod operations;
mod presentation;
mod resources;
mod scan;
mod scene;
mod types;
#[cfg(test)]
mod tests;

#[cfg(feature = "validation")]
pub(crate) use components::{FileList, FileManagerButton, FileNameInput, FileResizeHandle, FileRow, FileTaskButton, FileTitleBar};
pub(crate) use messages::{ExtraRequest, FileManagerCommand, FileManagerContextRequest, FileManagerExternalDrag, OpenFileWindow, OpenManagedFile};
pub(crate) use resources::{FileManagerState, FixedFileSelection};
pub(crate) use types::{ExtraAction, FileAction};

use bevy::prelude::*;

#[derive(SystemSet, Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub(crate) struct FileManagerInput;

pub(crate) fn install(app: &mut App) {
    browser::install(app);
    app.init_resource::<FileManagerState>().init_resource::<FixedFileSelection>()
        .add_message::<OpenFileWindow>().add_message::<OpenManagedFile>()
        .add_message::<FileManagerCommand>().add_message::<FileManagerExternalDrag>()
        .add_message::<FileManagerContextRequest>()
        .add_message::<ExtraRequest>()
        .add_systems(PreUpdate, input::track_pointer.after(bevy::ui::ui_focus_system))
        .add_systems(Update, (input::control, operations::control).chain()
            .after(bevy::text::apply_text_edits)
            .before(super::numeric_scrub_mouse_guard::update_scene_editor_numeric_scrub_mouse_guard)
            .in_set(FileManagerInput).in_set(crate::SceneEditorSystemSet::Input))
        .add_systems(Update, (scan::poll, operations::poll, scan::start).chain().in_set(crate::SceneEditorSystemSet::Tasks))
        .add_systems(Update, (scene::mount, presentation::sync).chain().in_set(crate::SceneEditorSystemSet::Presentation));
}
