//! 地表正文通过根应用注册的函数表构造，所有延迟命令仍进入原工作区队列。
use bevy::prelude::*;
use feathrise_editor_core::PresetWorkspaceTab;
pub(crate) use feathrise_editor_surface_model::spawn_surface_tabs;
use feathrise_editor_surface_model::{SurfaceEditorExtension, SurfacePresetEditorState};
pub(crate) fn spawn_active_surface_editor(
    commands: &mut Commands,
    host: Entity,
    font: &Handle<Font>,
    state: &SurfacePresetEditorState,
    active: PresetWorkspaceTab,
    extension: Option<&SurfaceEditorExtension>,
) -> bool {
    extension.is_some_and(|extension| (extension.spawn)(commands, host, font, state, active))
}
