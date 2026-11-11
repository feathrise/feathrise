//! 复用原工作台的开始、新建、打开标签及用户级默认工程目录设置。

mod about;
#[cfg(test)]
mod about_tests;
#[cfg(feature = "validation")]
mod about_validation;
#[cfg(feature = "validation")]
mod plugin_validation;
#[cfg(all(feature = "validation", windows))]
mod recent_validation;
#[cfg(feature = "validation")]
mod scale_validation;
mod components;
mod messages;
mod resources;
mod scene;
mod systems;
mod types;

pub(crate) use messages::WorkspaceCommand;
#[cfg(all(feature = "validation", windows))]
pub(crate) use recent_validation::install as install_recent_validation;
pub(crate) use resources::{AboutState, WorkspaceModel, WorkspaceProbe};
pub(crate) use types::{Page, RecentEntry, WorkspaceField, WorkspaceProject};

use bevy::prelude::*;

/// 后端在Input之后读取本帧意图，在Presentation之前发布工作快照。
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum WorkspaceUiSet {
    Input,
    Presentation,
}

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct AboutInput;

pub(crate) struct WorkspaceUiPlugin;

impl Plugin for WorkspaceUiPlugin {
    fn build(&self, app: &mut App) {
        let full_author = app.is_plugin_added::<feathrise_editor_scene::SceneEditorPlugin>();
        app.add_plugins(feathrise_editor_scene::workbench::WorkbenchHostPagesPlugin)
            .init_resource::<WorkspaceModel>()
            .init_resource::<WorkspaceProbe>()
            .init_resource::<resources::Presentation>()
            .init_resource::<AboutState>()
            .add_message::<WorkspaceCommand>()
            .configure_sets(
                Update,
                (WorkspaceUiSet::Input, WorkspaceUiSet::Presentation).chain(),
            )
            .configure_sets(
                Update,
                WorkspaceUiSet::Presentation
                    .after(feathrise_editor_core::SceneEditorSystemSet::Presentation),
            )
            .add_systems(Update, scene::mount.before(WorkspaceUiSet::Input))
            .add_systems(Update, about::mount.before(AboutInput))
            .add_systems(Update, about::input.in_set(AboutInput)
                .after(feathrise_editor_ui::update_editor_button_clicks)
                .after(crate::native_project_close::ProjectCloseInput)
                .before(crate::window_chrome::ChromeInput)
                .before(crate::viewport_mode::ViewportModeInput)
                .before(crate::project_log::ProjectLogInput)
                .before(feathrise_editor_scene::workbench::WorkbenchHostPageInput)
                .before(WorkspaceUiSet::Input)
                .before(feathrise_editor_ui::EditorUiSystemSet::NumericInput)
                .before(feathrise_editor_core::SceneEditorSystemSet::Input))
            .add_systems(Update, about::present.after(AboutInput))
            .add_systems(
                Update,
                (systems::adopt, systems::input)
                    .chain()
                    .in_set(WorkspaceUiSet::Input)
                    .after(crate::native_project_close::ProjectCloseInput)
                    .after(crate::native_plugins::management::PluginManagerInput)
                    .after(feathrise_editor_ui::update_editor_button_clicks)
                    .after(feathrise_editor_scene::workbench::WorkbenchHostPageInput)
                    .before(feathrise_editor_core::SceneEditorSystemSet::Input),
            )
            .add_systems(
                Update,
                (
                    systems::follow_native_navigation,
                    scene::recent_rows,
                    systems::present,
                    systems::disable_busy_inputs,
                    systems::cancel_visibility,
                    systems::open_dropped_project.after(crate::project_log::ProjectLogInput),
                    systems::scroll
                        .after(crate::project_log::ProjectLogInput)
                        .after(crate::native_project_close::ProjectCloseInput),
                )
                    .chain()
                    .in_set(WorkspaceUiSet::Presentation),
            )
            .add_systems(
                PostUpdate,
                systems::measure.after(bevy::ui::UiSystems::PostLayout),
            );
        install_text_boundary(app, full_author);
        #[cfg(feature = "validation")]
        about_validation::install(app);
        #[cfg(feature = "validation")]
        plugin_validation::install(app);
        #[cfg(feature = "validation")]
        scale_validation::install(app);
    }
}

fn install_text_boundary(app: &mut App, full_author: bool) {
    if full_author {
        // 完整作者App已由原text_input安装唯一官方处理器。资产文档以该系统类型排序，
        // 重复安装会使before/after目标歧义；工程UI只复用原处理结果。
        app.configure_sets(
            Update,
            WorkspaceUiSet::Input.after(bevy::text::apply_text_edits),
        );
    } else {
        // 无工程工作台没有SceneEditorPlugin，必须由宿主补齐这一次官方处理。
        app.add_systems(
            Update,
            bevy::text::apply_text_edits
                .in_set(WorkspaceUiSet::Input)
                .after(systems::adopt)
                .before(systems::input),
        );
    }
}

#[cfg(test)]
mod text_boundary_tests {
    use super::*;

    fn dependent_document_system() {}

    #[test]
    fn official_text_ordering_has_one_target_in_both_workspace_modes() {
        for full_author in [false, true] {
            let mut app = App::new();
            if full_author {
                app.add_systems(Update, bevy::text::apply_text_edits);
            }
            install_text_boundary(&mut app, full_author);
            app.add_systems(
                Update,
                dependent_document_system.after(bevy::text::apply_text_edits),
            );
            app.world_mut()
                .schedule_scope(Update, |world, schedule| {
                    // 只初始化真实调度图，不执行依赖窗口、字库和剪贴板的文本系统。
                    schedule
                        .initialize(world)
                        .expect("资产文档应始终能排序到唯一官方文本系统");
                });
        }
    }
}

/// 系统目录选择只回填对应字段，保留用户正在填写的其他表单值。
pub(crate) fn fill_path(world: &mut World, field: WorkspaceField, value: String) {
    let mut state = world.resource_mut::<resources::Presentation>();
    state.fills.retain(|(target, _)| *target != field);
    state.fills.push((field, value));
}

pub(crate) fn show_page(world: &mut World, page: Page) {
    systems::show(&mut world.resource_mut::<resources::Presentation>(), page);
    world
        .resource_mut::<feathrise_editor_scene::workbench::WorkbenchHostPageState>()
        .active = !matches!(page, Page::Editor);
}
