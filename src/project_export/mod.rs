//! 工程文件夹导出：保留作者现场，后台构建并将完整可移植包发布到工程 output 或所选目录。

pub(crate) mod messages;
pub(crate) mod resources;
mod package;
mod systems;
pub(crate) mod types;
pub(crate) mod ui;
mod components;
mod worker;
#[cfg(all(feature = "validation", windows))]
pub(crate) mod validation;
#[cfg(test)]
mod tests;

use bevy::prelude::*;
use std::path::PathBuf;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ExportInput;

pub(crate) struct ProjectExportPlugin {
    project: crate::project::Project,
    target: PathBuf,
}

impl ProjectExportPlugin {
    pub(crate) fn new(project: crate::project::Project, target: PathBuf) -> Self { Self { project, target } }
}

impl Plugin for ProjectExportPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(resources::ExportState::new(&self.project))
            .insert_resource(resources::ExportSettings { project: self.project.clone(), target: self.target.clone() })
            .init_non_send::<resources::ExportRuntime>()
            .add_message::<messages::OpenExportDialog>()
            .add_message::<messages::RequestExport>()
            .add_message::<messages::CancelExport>()
            .add_systems(Update, (systems::poll, systems::open, ui::mount).chain()
                .after(crate::native_plugins::management::PluginManagerInput).before(ExportInput))
            .add_systems(Update, (ui::handle, systems::cancel, systems::start).chain()
                .in_set(ExportInput)
                .after(bevy::text::apply_text_edits)
                .after(crate::native_project_close::ProjectCloseInput)
                // 日志先于插件管理；导出随后认领输入，不能再反向排在日志前形成调度环。
                .after(crate::native_plugins::management::PluginManagerInput)
                .before(crate::window_chrome::ChromeInput)
                .before(crate::workspace_ui::WorkspaceUiSet::Input)
                .before(crate::viewport_mode::ViewportModeInput)
                .before(feathrise_editor_ui::EditorUiSystemSet::NumericInput)
                .before(feathrise_editor_core::MapEditorSystemSet::Input))
            .add_systems(Update, ui::present.after(ExportInput));
        ui::install(app);
    }
}

/// 仅在原关闭事务获得明确授权之后取消；主线程等待的是非阻塞完成回执。
pub(crate) fn ready_to_exit(world: &mut World) -> Result<bool, String> {
    if !world.contains_non_send::<resources::ExportRuntime>() { return Ok(true); }
    ui::cancel_for_exit(world);
    {
        let mut runtime = world.non_send_mut::<resources::ExportRuntime>();
        runtime.stopping = true;
        if let Some(active) = &runtime.active { active.control.cancel(); }
    }
    systems::poll(world);
    let runtime = world.non_send::<resources::ExportRuntime>();
    if let Some(error) = &runtime.failure { return Err(error.clone()); }
    Ok(runtime.active.is_none() && ui::ready_to_exit(world))
}

pub(crate) fn build_profile_blocker(world: &World) -> Option<String> {
    world.get_resource::<resources::ExportState>().filter(|state| state.busy())
        .map(|_| "工程正在导出，请等待或取消后再切换构建配置".into())
}
