//! 关闭收尾与撤回后的代码服务接续；主线程只轮询真实线程退役，不等待源码扫描或构建。
use super::{backend::Backend, resources::*, types::CodeIterationPhase as Phase};
use bevy::prelude::*;
use feathrise_editor_core::ProjectBuildProfile;

/// 必须在旧线程已经结束后取最终代次；无论取消了几次都不能重置身份或绕过清理失败。
fn finish_retired(runtime: &mut CodeIterationRuntime) -> Result<bool, String> {
    if let Some(error) = &runtime.failure { return Err(error.clone()); }
    let Some(backend) = runtime.backend.as_ref() else { return Ok(true); };
    if !backend.is_finished() { return Ok(false); }
    runtime.last_generation = runtime.last_generation.max(backend.current_generation());
    if let Err(error) = runtime.backend.take().unwrap().finish() {
        runtime.failure = Some(error.clone());
        return Err(error);
    }
    Ok(true)
}

fn fail(state: &mut CodeIterationState, error: &str) {
    state.pending = None;
    state.phase = Phase::Failed;
    state.status = format!("代码：自动编译未恢复；{error}；作者草稿和编辑现场保留");
}

/// 只在原关闭事务已经撤回时认领恢复；普通确认取消及重复调用均不更换后台。
pub(crate) fn cancel_exit(world: &mut World) {
    // 缺少关闭状态不能证明原关闭事务已经撤回；正常确认取消仍不更换后台。
    if !world.get_resource::<crate::native_project_close::NativeProjectCloseState>()
        .is_some_and(|close| !close.active()) { return; }
    let Some(mut runtime) = world.get_non_send_mut::<CodeIterationRuntime>() else { return; };
    if runtime.lifecycle != BackendLifecycle::Closing { return; }
    runtime.lifecycle = BackendLifecycle::Resuming;
    drop(runtime);
    if let Some(mut state) = world.get_resource_mut::<CodeIterationState>() {
        state.pending = None;
        super::systems::clear_diagnostics(&mut state);
        state.phase = Phase::Waiting;
        state.status = "代码：正在恢复自动编译，等待上次构建安全结束".into();
    }
}

/// 原退出意图唯一入口；再次确认关闭会撤销恢复，已取消的任务仍由原槽继续收口。
pub(crate) fn ready_to_exit(world: &mut World) -> Result<bool, String> {
    #[cfg(all(feature = "validation", windows))]
    let validation_hold = super::validation::close_drain_held(world);
    let Some(mut runtime) = world.get_non_send_mut::<CodeIterationRuntime>() else { return Ok(true); };
    runtime.lifecycle = BackendLifecycle::Closing;
    if let Some(backend) = runtime.backend.as_ref() { backend.cancel(); }
    let result = finish_retired(&mut runtime);
    drop(runtime);
    if let Some(mut state) = world.get_resource_mut::<CodeIterationState>() {
        state.pending = None;
        if let Err(error) = &result { fail(&mut state, error); }
        else if state.phase != Phase::Stopping {
            state.phase = Phase::Stopping;
            state.status = "代码：正在结束本次构建与源码监听".into();
        }
    }
    #[cfg(all(feature = "validation", windows))]
    if validation_hold && matches!(result, Ok(true)) { return Ok(false); }
    result
}

/// 返回true才消费正常构建回执；恢复期间旧实例仍占唯一槽，后台补偿首扫覆盖停监听保存。
pub(super) fn advance(
    runtime: &mut CodeIterationRuntime, state: &mut CodeIterationState, profile: ProjectBuildProfile,
) -> bool {
    match runtime.lifecycle {
        BackendLifecycle::Running => return true,
        BackendLifecycle::Closing => { state.phase = Phase::Stopping; return false; }
        BackendLifecycle::Resuming => (),
    }
    match finish_retired(runtime) {
        Ok(false) => return false,
        Err(error) => { fail(state, &error); return false; }
        Ok(true) => (),
    }
    let settings = &runtime.settings;
    match Backend::spawn_resumed(settings.project.clone(), settings.target.clone(), profile,
        settings.logs.clone(), runtime.last_generation)
    {
        Ok(backend) => {
            state.generation = backend.current_generation();
            state.pending = None;
            super::systems::clear_diagnostics(state);
            state.phase = Phase::Waiting;
            state.status = "代码：已恢复源码监听，正在重新编译最新已保存内容".into();
            runtime.profile = profile;
            runtime.backend = Some(backend);
            runtime.lifecycle = BackendLifecycle::Running;
            true
        }
        Err(error) => { runtime.failure = Some(error.clone()); fail(state, &error); false }
    }
}
