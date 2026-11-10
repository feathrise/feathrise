//! 独立地表预设实现：输入、中央表单和目录写入与场景宿主并列装配。
mod components;
mod fields;
mod helpers;
mod interaction;
mod persistence;
mod project_close;
pub use project_close::request_project_save;
mod presentation;
mod types;
mod widgets;
mod workspace;
use bevy::prelude::*;
use feathrise_editor_surface_model::{SurfaceEditorExtension, SurfacePresetEditorState};
pub struct SurfaceEditorPlugin;
impl Plugin for SurfaceEditorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SurfacePresetEditorState>()
            .insert_resource(extension());
    }
}
/// 真实宿主夹具也使用同一函数表；不复制私有实现或建立第二套调度。
pub fn extension() -> SurfaceEditorExtension {
    SurfaceEditorExtension {
        buttons: interaction::handle_buttons,
        text: interaction::handle_text_input,
        persistence: persistence::poll_catalog_write,
        panel: presentation::sync_panel,
        badge: presentation::sync_document_badge,
        styles: presentation::sync_button_styles,
        spawn: workspace::spawn_active_surface_editor,
    }
}
