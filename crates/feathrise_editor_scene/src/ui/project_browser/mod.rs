//! 单一工程目录入口；资产子目录沿用原资产链，源码只交给原文档会话。

mod components;
mod filter;
mod file_manager_bridge;
mod file_menu;
mod input;
pub(crate) mod navigation;
mod popup;
mod presentation;
mod resources;
mod scene;
mod types;
mod window;
#[cfg(test)]
mod gpu_tests;

pub(crate) use components::ProjectBrowserContent;
pub(crate) use components::ProjectFileRow; // 共用路径菜单仅读取可见行绑定身份，不持有整个工程目录。
pub(crate) use filter::filter_toolbar;
#[cfg(feature = "validation")]
pub(crate) use {components::ProjectFileFilter, filter::ProjectTypeFilterButton};
pub(crate) use popup::{ProjectBrowserPopup, spawn_expander};
#[cfg(feature = "validation")]
pub(crate) use popup::{ProjectAnimationClip, ProjectAnimationExpand, ProjectTypeFilterOption};
pub(crate) use resources::{ProjectBrowserState, ProjectFileViewport};
pub(crate) use scene::{files_panel, parent_directory_row};
pub(in crate::ui) use scene::{asset_folder_path, refresh_button, toolbar_button};
pub(crate) use types::ProjectBrowserView;

use bevy::prelude::*;

/// 根身份、内部来源与目录采用先完成，文档桥随后才验证同帧打开意图。
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ProjectBrowserTaskSet;

/// 实际目录采用与跨资产根路由完成后，文档打开再核对最终来源。
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ProjectBrowserRoutingSet;

/// 文档等独立GPU夹具复用正式来源显隐，避免只建左栏却把真正的资产正文留在隐藏状态。
#[cfg(test)]
pub(crate) fn install_asset_view_for_tests(app: &mut App) {
    app.insert_resource(ProjectBrowserState { view: ProjectBrowserView::Assets, ..default() })
        .add_systems(Update, presentation::views.in_set(crate::SceneEditorSystemSet::Presentation));
}

pub(crate) fn install(app: &mut App) {
    use crate::{SceneEditorSystemSet, asset_catalog::project_files};
    app.init_resource::<ProjectBrowserState>()
        .init_resource::<ProjectBrowserPopup>()
        .init_resource::<ProjectFileViewport>()
        .init_resource::<project_files::EditorProjectFileCatalog>()
        .add_systems(Update, navigation::asset_root_up
            .after(crate::ui::numeric_scrub_mouse_guard::update_scene_editor_numeric_scrub_mouse_guard)
            .before(crate::ui::actions::handle_editor_action_buttons)
            .in_set(SceneEditorSystemSet::Input))
        .add_systems(Update, popup::control
            .before(crate::ui::numeric_scrub_mouse_guard::update_scene_editor_numeric_scrub_mouse_guard)
            .in_set(SceneEditorSystemSet::Input))
        .add_systems(Update, popup::publish_clip
            .after(popup::control)
            .after(crate::ui::numeric_scrub_mouse_guard::update_scene_editor_numeric_scrub_mouse_guard)
            .before(crate::ui::asset_preview::ProjectAnimationPickSet)
            .in_set(SceneEditorSystemSet::Input))
        .add_systems(Update, (filter::edit_name_filter, input::files).chain()
            .after(bevy::text::apply_text_edits)
            .after(crate::ui::numeric_scrub_mouse_guard::update_scene_editor_numeric_scrub_mouse_guard)
            .in_set(SceneEditorSystemSet::Input))
        .add_systems(Update, (crate::workbench::assets::bind_project_files_source,
            input::update_active, project_files::start_project_file_scan, project_files::poll_project_file_scan).chain()
            .in_set(ProjectBrowserTaskSet).in_set(SceneEditorSystemSet::Tasks))
        .add_systems(Update, navigation::follow_directory
            .after(ProjectBrowserTaskSet)
            .after(crate::asset_catalog::poll_asset_scan)
            .in_set(ProjectBrowserRoutingSet).in_set(SceneEditorSystemSet::Tasks))
        .add_systems(Update, popup::mount.in_set(SceneEditorSystemSet::Tasks))
        .add_systems(Update, (window::prepare, presentation::mount_rows, presentation::views, presentation::files, presentation::action_feedback).chain()
            .after(crate::ui::systems::scroll_editor_side_panels)
            .in_set(SceneEditorSystemSet::Presentation))
        .add_systems(Update, (filter::sync_toolbar, popup::bind_expanders, popup::show)
            .after(presentation::views)
            .after(crate::ui::rows::sync_asset_rows)
            .in_set(SceneEditorSystemSet::Presentation))
        .add_systems(Update, presentation::asset_folder
            .after(crate::ui::systems::sync_editor_shell)
            .in_set(SceneEditorSystemSet::Presentation));
}

/// 文件桌面负责装配这些桥接系统和消息；无工程的轻量工作台仅复用原浏览器。
pub(super) fn install_file_manager_bridge(app: &mut App) {
    use crate::{SceneEditorSystemSet, ui::file_manager::FileManagerInput};
    app.add_systems(Update, (file_manager_bridge::sync_fixed_selection, file_manager_bridge::fixed_shortcuts).chain()
            .before(FileManagerInput)
            .in_set(SceneEditorSystemSet::Input))
        .add_systems(Update, file_manager_bridge::refresh_fixed_after_operation
            .before(ProjectBrowserTaskSet).in_set(SceneEditorSystemSet::Tasks))
        .add_systems(Update, file_menu::dispatch
            .after(crate::ui::file_manager::browser::BrowserInput)
            .in_set(SceneEditorSystemSet::Input));
}
