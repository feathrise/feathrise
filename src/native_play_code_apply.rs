//! 工程运行采用已完整构建的独立版本；只替换游戏Job，作者文档与历史始终留在原World。

use super::{
    resources::{ActivePlay, NativePlayRuntime, NativePlaySettings, PendingCodeApply},
    CodeApplyState, CodeApplyStatus, NativePlayStatus, PreparedGame,
};
use bevy::prelude::*;
use feathrise_editor_map::standalone::{SavedPlayFeedback, StandaloneEditorMode};
use std::{sync::{Arc, mpsc}, thread};

pub(crate) fn code_apply_status(world: &World) -> Option<CodeApplyStatus> {
    world.get_non_send::<NativePlayRuntime>()?.code_apply_status.clone()
}

/// 新源码只撤销尚未启动的候选；已启动的独立版本继续作为明确的旧运行版本保留。
pub(crate) fn cancel_pending_code_apply(world: &mut World, token: u64, reason: &str) -> bool {
    let Some(mut runtime) = world.get_non_send_mut::<NativePlayRuntime>() else { return false; };
    if !runtime.code_apply_pending.as_ref().is_some_and(|pending| pending.token == token) { return false; }
    runtime.code_apply_pending = None;
    fail(&mut runtime, reason);
    true
}

/// 共用原保存和作者事务门。只在明确点击及真正重开前检查，不逐帧扫描文本正文。
pub(crate) fn code_apply_blocker(world: &World) -> Option<String> {
    if world.get_resource::<crate::native_project_close::NativeProjectCloseState>()
        .is_some_and(|close| close.active())
    {
        return Some("请先完成或取消关闭工程，再应用新代码".into());
    }
    if world.get_resource::<crate::native_project_menu::NativeProjectMenuState>()
        .is_some_and(|menu| menu.saving())
        || feathrise_editor_map::standalone::viewport_projection::switch_pending(world)
    {
        return Some("请先等待保存或地图切换完成，再应用新代码".into());
    }
    if let Some(reason) = feathrise_editor_map::standalone::saved_play_blocker(world) {
        return Some(reason);
    }
    let snapshot = feathrise_editor_map::standalone::project_close_snapshot(world);
    snapshot.blocked.or(snapshot.pending)
}

/// token由代码迭代层绑定完整构建身份；本入口只接受本工程当前配置的已发布独立产物。
pub(crate) fn request_code_apply(
    world: &mut World,
    prepared: Arc<PreparedGame>,
    token: u64,
) -> Result<(), String> {
    if token == 0 { return Err("代码应用身份必须非零".into()); }
    if crate::embedded_viewport::owns_play(world) {
        return Err("当前使用工程3D，请在预览中应用版本".into());
    }
    if let Some(reason) = code_apply_blocker(world) { return Err(reason); }
    let profile = world.get_resource::<feathrise_editor_core::ProjectLaunchSettings>()
        .map_or(default(), |settings| settings.profile);
    let build_backend = super::ProjectBuildBackend::selected(world);
    let settings = world.resource::<NativePlaySettings>();
    if !prepared.matches_project(&settings.project, profile) || !prepared.matches_backend(build_backend) {
        return Err("新代码产物与当前工程、构建配置或构建方式不一致，本次未应用".into());
    }
    let request = world.non_send::<NativePlayRuntime>().active_request.clone()
        .unwrap_or_else(|| feathrise_editor_map::standalone::current_saved_play_request(world, profile));
    let mut runtime = world.non_send_mut::<NativePlayRuntime>();
    if runtime.cleanup_failed || runtime.exit_requested {
        return Err("当前工程实例或工具退出尚未确认，暂不能应用新版本".into());
    }
    if runtime.code_apply_pending.is_some()
        || runtime.code_apply_status.as_ref().is_some_and(|apply| apply.state == CodeApplyState::Applying)
    {
        return Err("已有新版本正在应用，请等待本次实际启动回执".into());
    }
    if runtime.active.as_ref().is_some_and(|active| active.control.is_cancelled()) {
        return Err("当前工程运行尚未结束，请稍后再应用新版本".into());
    }
    runtime.code_apply_pending = Some(PendingCodeApply { prepared, request, token });
    runtime.code_apply_status = Some(CodeApplyStatus { token, state: CodeApplyState::Applying, error: None });
    if let Some(active) = runtime.active.as_ref() { active.control.cancel(); }
    if let Some(mut status) = world.get_resource_mut::<NativePlayStatus>() { status.preparing = false; }
    world.write_message(SavedPlayFeedback("正在结束原工程运行并应用新代码；未保存草稿仍留在编辑器".into()));
    Ok(())
}

pub(super) fn fail(runtime: &mut NativePlayRuntime, error: &str) {
    if let Some(apply) = runtime.code_apply_status.as_mut() {
        apply.state = CodeApplyState::Failed;
        apply.error = Some(error.into());
    }
}

pub(super) fn advance(world: &mut World) {
    if world.non_send::<NativePlayRuntime>().code_apply_pending.is_none() { return; }
    let blocked: Option<String> = if world.non_send::<NativePlayRuntime>().cleanup_failed {
        Some("原运行进程或通信线程退出未确认，禁止启动新版本".into())
    } else if world.non_send::<NativePlayRuntime>().exit_requested {
        Some("工程正在退出，本次代码应用已取消".into())
    } else if world.get_resource::<crate::native_project_close::NativeProjectCloseState>()
        .is_some_and(|close| close.active())
    {
        Some("工程关闭已接管，本次代码应用已取消".into())
    } else { None };
    if let Some(reason) = blocked {
        let mut runtime = world.non_send_mut::<NativePlayRuntime>();
        runtime.code_apply_pending = None;
        fail(&mut runtime, &reason);
        world.write_message(SavedPlayFeedback(reason));
        return;
    }
    if world.non_send::<NativePlayRuntime>().active.is_some() { return; }
    if let Some(reason) = code_apply_blocker(world) {
        let mut runtime = world.non_send_mut::<NativePlayRuntime>();
        runtime.code_apply_pending = None;
        fail(&mut runtime, &reason);
        world.write_message(SavedPlayFeedback(reason));
        return;
    }
    if crate::embedded_viewport::owns_play(world) {
        let mut runtime = world.non_send_mut::<NativePlayRuntime>();
        runtime.code_apply_pending = None;
        fail(&mut runtime, "3D方式已经改变，本次版本应用已取消");
        return;
    }
    let pending = world.non_send_mut::<NativePlayRuntime>().code_apply_pending.take().unwrap();
    let project = world.resource::<NativePlaySettings>().project.clone();
    let build_backend = super::ProjectBuildBackend::selected(world);
    // 等待旧进程退出期间选择也可能失效，必须在创建新Job前再次核对完整启动身份。
    if !pending.prepared.matches_project(&project, pending.request.profile)
        || !pending.prepared.matches_backend(build_backend) {
        fail(&mut world.non_send_mut::<NativePlayRuntime>(), "产物工程身份或构建方式变化，未启动新版本");
        return;
    }
    let control = world.get_resource::<crate::project_log::ProjectLogChannel>()
        .map(|logs| crate::build_process::BuildControl::with_log(logs.begin_task(crate::project_log::LogTaskKind::SavedPlay)))
        .unwrap_or_default();
    let (progress_tx, progress_rx) = mpsc::sync_channel(4);
    let (result_tx, result_rx) = mpsc::sync_channel(1);
    let (started_tx, started_rx) = mpsc::sync_channel(1);
    let worker_control = control.clone();
    let request = pending.request.clone();
    let prepared = pending.prepared;
    let handle = thread::spawn(move || {
        let result = super::worker::run_prepared(project, prepared, request, build_backend, &worker_control, &progress_tx, Some(started_tx));
        let _ = result_tx.send(result);
    });
    let mut runtime = world.non_send_mut::<NativePlayRuntime>();
    runtime.active = Some(ActivePlay { control, progress_rx, result_rx, handle });
    runtime.active_request = Some(pending.request);
    runtime.code_apply_active_token = Some(pending.token);
    runtime.code_apply_started = Some(started_rx);
    runtime.launch_started = None;
    let mut status = world.resource_mut::<NativePlayStatus>();
    status.started = status.started.saturating_add(1);
    status.active = true;
    status.preparing = true;
    status.last_result = None;
    status.last_exit_code = None;
    if let Some(mut mode) = world.get_resource_mut::<StandaloneEditorMode>() {
        mode.set_external_play_active(true);
    }
    world.write_message(SavedPlayFeedback("正在启动已完整构建的新版本；运行实例仍读取已保存地图，作者草稿和历史保留".into()));
}
