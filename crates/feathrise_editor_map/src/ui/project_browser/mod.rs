//! 单一工程目录入口；资产子目录沿用原资产链，源码只交给原文档会话。

mod components;
mod input;
pub(crate) mod navigation;
mod presentation;
mod resources;
mod scene;
mod types;
mod window;
#[cfg(test)]
mod gpu_tests;

pub(crate) use components::ProjectBrowserContent;
pub(crate) use resources::{ProjectBrowserState, ProjectFileViewport};
pub(crate) use scene::files_panel;
pub(crate) use types::ProjectBrowserView;

use bevy::prelude::*;

/// 根身份、内部来源与目录采用先完成，文档桥随后才验证同帧打开意图。
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ProjectBrowserTaskSet;

/// 实际目录采用与跨资产根路由完成后，文档打开再核对最终来源。
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ProjectBrowserRoutingSet;

pub(crate) fn install(app: &mut App) {
    use crate::{MapEditorSystemSet, asset_catalog::project_files};
    app.init_resource::<ProjectBrowserState>()
        .init_resource::<ProjectFileViewport>()
        .init_resource::<project_files::EditorProjectFileCatalog>()
        .add_systems(Update, navigation::asset_root_up
            .after(crate::ui::numeric_scrub_mouse_guard::update_map_editor_numeric_scrub_mouse_guard)
            .before(crate::ui::actions::handle_editor_action_buttons)
            .in_set(MapEditorSystemSet::Input))
        .add_systems(Update, input::files
            .after(bevy::text::apply_text_edits)
            .after(crate::ui::numeric_scrub_mouse_guard::update_map_editor_numeric_scrub_mouse_guard)
            .in_set(MapEditorSystemSet::Input))
        .add_systems(Update, (crate::workbench::assets::bind_project_files_source,
            input::update_active, project_files::start_project_file_scan, project_files::poll_project_file_scan).chain()
            .in_set(ProjectBrowserTaskSet).in_set(MapEditorSystemSet::Tasks))
        .add_systems(Update, navigation::follow_directory
            .after(ProjectBrowserTaskSet)
            .after(crate::asset_catalog::poll_asset_scan)
            .in_set(ProjectBrowserRoutingSet).in_set(MapEditorSystemSet::Tasks))
        .add_systems(Update, (window::prepare, presentation::mount_rows, presentation::views, presentation::files, presentation::action_feedback).chain()
            .after(crate::ui::systems::scroll_editor_side_panels)
            .in_set(MapEditorSystemSet::Presentation))
        .add_systems(Update, presentation::asset_folder
            .after(crate::ui::systems::sync_editor_shell)
            .in_set(MapEditorSystemSet::Presentation));
}
