//! 在原作者工作台顶栏附加工程入口；只保存启动设置或请求原退出门，不重做作者界面。

#[path = "native_project_menu_components.rs"]
mod components;
#[path = "native_project_menu_resources.rs"]
mod resources;
#[path = "native_project_menu_store.rs"]
mod store;
#[path = "native_project_menu_systems.rs"]
mod systems;
#[path = "native_project_menu_ui.rs"]
mod ui;

pub(crate) use resources::NativeProjectMenuState;

use bevy::prelude::*;

/// 同窗口工程菜单复用原启动场景保存槽，关闭仍等待同一原事务，不另写工程文件。
pub(crate) fn save_startup_from_workspace(world: &mut World) {
    if world.resource::<NativeProjectMenuState>().saving() { return; }
    let id = world.get_resource::<feathrise_scene_content::ActiveDynamicScene>()
        .and_then(|active| active.scene_id.as_ref()).map(|id| id.0.clone());
    let Some(id) = id else {
        world.write_message(feathrise_editor_scene::standalone::SavedPlayFeedback("请先进入场景，再设置启动场景".into()));
        return;
    };
    let project = world.resource::<resources::ProjectMenuSettings>().project.clone();
    let mut state = world.resource_mut::<NativeProjectMenuState>();
    state.failed = None;
    state.status = "正在保存当前启动场景…".into();
    state.task = Some(bevy::tasks::IoTaskPool::get().spawn(async move { store::save(&project, &id) }));
}

pub(crate) struct NativeProjectMenuPlugin {
    project: crate::project::Project,
}

impl NativeProjectMenuPlugin {
    pub(crate) fn new(project: crate::project::Project) -> Self {
        Self { project }
    }
}

impl Plugin for NativeProjectMenuPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(resources::ProjectMenuSettings {
            project: self.project.clone(),
            managed: std::env::var_os("NEW_EDITOR_MANAGED_INSTANCE")
                .is_some_and(|value| value == "1"),
        })
        .init_resource::<NativeProjectMenuState>()
        .init_resource::<feathrise_editor_core::EditorHostOverlay>()
        .add_systems(Update, ui::mount.before(systems::present))
        .add_systems(
            Update,
            systems::handle
                .after(crate::workspace_ui::AboutInput)
                .after(crate::native_project_close::ProjectCloseInput)
                .after(crate::project_export::ExportInput)
                .after(crate::project_log::ProjectLogInput)
                .after(crate::viewport_mode::ViewportModeInput)
                .after(crate::code_iteration::diagnostics_ui::DiagnosticsInput)
                .after(crate::native_plugins::management::PluginManagerInput)
                .before(crate::native_play::request_exit)
                .before(feathrise_editor_ui::EditorUiSystemSet::NumericInput)
                .before(feathrise_editor_core::SceneEditorSystemSet::Input),
        )
        .add_systems(
            Update,
            (systems::poll, systems::present)
                .chain()
                .after(systems::handle),
        )
        .add_systems(
            Update,
            systems::emit_close.after(feathrise_editor_core::SceneEditorSystemSet::Input),
        );
    }
}
