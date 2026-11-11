//! 工程代码迭代第一阶段：后台编译、诊断定位与显式版本应用，作者World保持原位。

mod backend;
mod components;
pub(crate) mod hotpatch;
pub(crate) mod diagnostics_ui;
mod lifecycle;
mod resources;
mod systems;
mod types;
mod ui;
#[cfg(all(feature = "validation", windows))]
pub(crate) mod validation;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod lifecycle_tests;

pub(crate) use resources::CodeIterationState;
#[cfg(feature = "validation")]
pub(crate) use types::CodeIterationPhase;
pub(crate) use backend::{Backend, BuildIdentity, Event, PreparedVersion};
pub(crate) use backend::export_source_identity;
pub(crate) use backend::{PreparedArtifact, capture_source};
#[cfg(test)]
pub(crate) use backend::SourceSnapshot;
use resources::{BackendLifecycle, BackendSettings, CodeIterationRuntime};
pub(crate) use lifecycle::{cancel_exit, ready_to_exit};
use bevy::prelude::*;
use std::path::PathBuf;

pub(crate) fn export_input_guard(world: &World) -> Option<crate::build_process::InputGuard> {
    world.get_non_send::<CodeIterationRuntime>()?.backend.as_ref().map(Backend::export_input_guard)
}

pub(crate) fn export_output_registrar(world: &World) -> Option<crate::build_process::OutputRegistrar> {
    world.get_non_send::<CodeIterationRuntime>()?.backend.as_ref().map(Backend::export_output_registrar)
}

/// 恢复期间不能省略监听租约后启动导出；未安装代码迭代的独立测试App保持原调用合同。
pub(crate) fn export_blocker(world: &World) -> Option<String> {
    let runtime = world.get_non_send::<CodeIterationRuntime>()?;
    if let Some(error) = &runtime.failure { return Some(format!("代码构建收口未确认，暂不导出：{error}")); }
    if runtime.lifecycle != BackendLifecycle::Running { return Some("代码监听正在收口或恢复，请等待完成后再导出".into()); }
    let Some(backend) = runtime.backend.as_ref() else { return Some("代码监听尚未就绪，请等待恢复后再导出".into()); };
    if backend.cleanup_failed() || backend.is_finished() || !(backend.export_input_guard().0)() {
        return Some("代码监听已停止或构建收口未确认，请重新打开工程后再导出".into());
    }
    None
}

#[derive(Message, Clone, Copy)]
pub(crate) struct RequestApplyCode {
    /// 点击时冻结已显示版本；后台再完成新版本也不能把旧按钮释放改派给它。
    pub(crate) generation: u64,
}

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct CodeIterationInput;

pub(crate) struct CodeIterationPlugin {
    project: crate::project::Project,
    target: PathBuf,
}

impl CodeIterationPlugin {
    pub(crate) fn new(project: crate::project::Project, target: PathBuf) -> Self { Self { project, target } }
}

impl Plugin for CodeIterationPlugin {
    fn build(&self, app: &mut App) {
        let profile = app.world().get_resource::<feathrise_editor_core::ProjectLaunchSettings>()
            .map_or(feathrise_editor_core::ProjectBuildProfile::default(), |launch| launch.profile);
        let logs = app.world().get_resource::<crate::project_log::ProjectLogChannel>().cloned();
        app.insert_resource(CodeIterationState::new(&self.project))
            .insert_non_send(CodeIterationRuntime {
                backend: Some(Backend::spawn(self.project.clone(), self.target.clone(), profile, logs.clone())),
                settings: BackendSettings { project: self.project.clone(), target: self.target.clone(), logs },
                profile, lifecycle: BackendLifecycle::Running, last_generation: 1, failure: None,
            })
            .add_message::<RequestApplyCode>()
            .add_systems(Update, (systems::sync_patch_context, systems::poll).chain()
                .after(crate::viewport_mode::ViewportModeInput).before(CodeIterationInput))
            .add_systems(Update, ui::mount.before(CodeIterationInput))
            .add_systems(Update, (systems::guard_application, ui::input, systems::apply).chain()
                .in_set(CodeIterationInput)
                .after(crate::viewport_mode::ViewportModeInput)
                .after(crate::native_project_close::ProjectCloseInput)
                .before(diagnostics_ui::DiagnosticsInput)
                .before(feathrise_editor_scene::standalone::SavedPlayDispatch))
            .add_systems(Update, systems::application_result.after(CodeIterationInput)
                .before(feathrise_editor_core::SceneEditorSystemSet::Presentation))
            .add_systems(Update, ui::present.after(systems::application_result))
            .add_systems(PostUpdate, ui::status
                .in_set(feathrise_editor_ui::feedback::PanelStatusProjection)
                .before(bevy::ui::UiSystems::Prepare));
        diagnostics_ui::install(app);
        #[cfg(all(feature = "validation", windows))]
        validation::install(app, &self.project, &self.target);
    }
}

/// 宿主发送补丁前最后核对原子源码代次；主帧不获取后台互斥锁或重扫源码。
pub(crate) fn generation_is_current(world: &World, generation: u64) -> bool {
    world.get_non_send::<CodeIterationRuntime>().and_then(|runtime| runtime.backend.as_ref())
        .is_some_and(|backend| world.get_resource::<feathrise_editor_core::ProjectLaunchSettings>()
            .is_some_and(|launch| backend.generation_is_current(generation, launch.profile)))
}
