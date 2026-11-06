//! 新代码只重启精确持有的游戏Job；重用原作者快照、相机和可靠模式事务恢复工作现场。

use super::resources::{CodeApply, CodeApplyStage, EmbeddedStatus, Runtime, Settings};
use crate::{native_play::{CodeApplyState, CodeApplyStatus, PreparedGame}, viewport_mode::ViewportMode};
use bevy::prelude::*;
use feathrise_editor_protocol::Mode;
use feathrise_editor_map::standalone::embedded_snapshot::{self, EmbeddedSnapshotState};
use std::{sync::Arc, time::{Duration, Instant}};

pub(crate) fn code_apply_status(world: &World) -> Option<CodeApplyStatus> {
    world.get_non_send::<Runtime>()?.code_apply_status.clone()
}

/// 仅回到编辑或收口旧Job的阶段仍可撤回；新会话已开始后不因后台新保存自动杀掉它。
pub(crate) fn cancel_pending_code_apply(world: &mut World, token: u64, reason: &str) -> bool {
    if super::hotpatch::cancel_pending(world, token, reason) { return true; }
    let Some(mut runtime) = world.get_non_send_mut::<Runtime>() else { return false; };
    if !runtime.code_apply.as_ref().is_some_and(|apply| apply.token == token
        && matches!(apply.stage, CodeApplyStage::Returning | CodeApplyStage::Draining)) { return false; }
    runtime.pending_edit = false;
    fail(&mut runtime, reason);
    true
}

/// 只记录已经完整构建的候选；不在点击帧检查文件或复制运行库，也不重建作者World。
pub(crate) fn request_code_apply(
    world: &mut World,
    prepared: Arc<PreparedGame>,
    token: u64,
) -> Result<(), String> {
    if token == 0 { return Err("代码应用身份必须非零".into()); }
    if world.resource::<crate::viewport_mode::ViewportModeState>().selected != ViewportMode::EmbeddedWorld {
        return Err("当前没有选择工程3D，不能启动常驻应用事务".into());
    }
    if world.get_resource::<crate::native_play::NativePlayStatus>().is_some_and(|status| status.active) {
        return Err("原工程运行尚未结束，请先确认运行进程退出".into());
    }
    if let Some(reason) = crate::native_play::code_apply_blocker(world) { return Err(reason); }
    if let Some(reason) = super::systems::play_blocker(world) { return Err(reason); }
    let profile = world.get_resource::<feathrise_editor_core::ProjectLaunchSettings>()
        .map_or(default(), |settings| settings.profile);
    if !prepared.matches_project(&world.resource::<Settings>().project, profile)
        || !prepared.matches_backend(crate::native_play::ProjectBuildBackend::selected(world)) {
        return Err("新代码产物与当前工程或构建配置不一致，本次未应用".into());
    }
    let status = world.resource::<EmbeddedStatus>();
    let resume_playing = status.playing;
    let starting = status.active && !status.ready;
    let mut runtime = world.non_send_mut::<Runtime>();
    if let Some(error) = &runtime.cleanup_failure { return Err(error.clone()); }
    if runtime.exit_requested || runtime.close_return_requested {
        return Err("工程正在关闭，请先取消关闭再应用新代码".into());
    }
    if starting || runtime.waiting_mode.is_some() || runtime.pending_edit || runtime.pending_play
        || runtime.restart_requested || runtime.code_apply.is_some() || runtime.hotpatch.flight.is_some()
    {
        return Err("当前构建、模式交接或代码应用尚未完成，请稍后重试".into());
    }
    if runtime.active.as_ref().is_some_and(|active| active.control.is_cancelled()) {
        return Err("正在确认当前运行Job退出，请稍后再应用新代码".into());
    }
    runtime.code_apply = Some(CodeApply {
        prepared, token, resume_playing,
        stage: if resume_playing { CodeApplyStage::Returning } else { CodeApplyStage::Draining },
        started: Instant::now(), session: None, restored_revision: None,
        mode_confirmed: false, progress: None,
    });
    runtime.code_apply_status = Some(CodeApplyStatus { token, state: CodeApplyState::Applying, error: None });
    world.write_message(feathrise_editor_map::standalone::SavedPlayFeedback(
        "正在应用新代码；重启运行实例后重新采用当前草稿并恢复原预览模式，作者历史保留".into(),
    ));
    Ok(())
}

fn fail(runtime: &mut Runtime, error: &str) {
    if let Some(apply) = runtime.code_apply.take() {
        runtime.code_apply_status = Some(CodeApplyStatus { token: apply.token, state: CodeApplyState::Failed, error: Some(error.into()) });
        runtime.pending_play = false;
    }
}

pub(super) fn ready_to_start(runtime: &Runtime) -> bool {
    runtime.code_apply.as_ref().is_some_and(|apply| apply.stage == CodeApplyStage::Draining)
}

pub(super) fn before_start(
    world: &mut World,
    runtime: &mut Runtime,
    status: &mut EmbeddedStatus,
    selected: ViewportMode,
    profile: feathrise_editor_core::ProjectBuildProfile,
) {
    let Some(apply) = runtime.code_apply.as_ref() else { return; };
    let blocked = if selected != ViewportMode::EmbeddedWorld {
        Some("3D方式已经改变，本次常驻代码应用已取消".to_owned())
    } else if runtime.exit_requested || runtime.close_return_requested
        || world.get_resource::<crate::native_project_close::NativeProjectCloseState>().is_some_and(|close| close.active())
    {
        Some("工程关闭已接管，本次代码应用已取消；作者现场保留".into())
    } else if let Some(error) = &runtime.cleanup_failure {
        Some(error.clone())
    } else if !apply.prepared.matches_project(&world.resource::<Settings>().project, profile)
        || !apply.prepared.matches_backend(crate::native_play::ProjectBuildBackend::selected(world)) {
        Some("当前工程或构建配置已变化，未继续应用旧候选版本".into())
    } else if matches!(apply.stage, CodeApplyStage::Starting | CodeApplyStage::Synchronizing | CodeApplyStage::Restoring)
        && (runtime.active.is_none() || status.error.is_some())
    {
        Some(status.error.clone().unwrap_or_else(|| "新代码运行实例在恢复当前草稿前退出；可再次应用重试".into()))
    } else { None };
    if let Some(error) = blocked {
        if let Some(active) = &runtime.active { active.control.cancel(); }
        fail(runtime, &error);
        super::systems::feedback(world, &error);
        return;
    }
    let apply = runtime.code_apply.as_mut().unwrap();
    if apply.stage == CodeApplyStage::Returning {
        if runtime.active.is_none() || (!status.playing && runtime.waiting_mode.is_none()) {
            apply.stage = CodeApplyStage::Draining;
            runtime.pending_edit = false;
        } else if apply.started.elapsed() >= Duration::from_secs(5) {
            // 控制邮箱堵塞也有独立预算；仍由poll确认旧Job退出，不能提前改变作者呈现。
            apply.stage = CodeApplyStage::Draining;
            runtime.pending_edit = false;
        } else {
            runtime.pending_edit = true;
        }
    }
    if apply.stage == CodeApplyStage::Draining {
        if let Some(active) = &runtime.active { active.control.cancel(); }
    } else if apply.stage == CodeApplyStage::Starting && status.ready {
        apply.stage = CodeApplyStage::Synchronizing;
        apply.started = Instant::now();
    }
}

pub(super) fn started(runtime: &mut Runtime) {
    let session = runtime.identity.as_ref().map(|identity| identity.session);
    let apply = runtime.code_apply.as_mut().unwrap();
    apply.stage = CodeApplyStage::Starting;
    apply.session = session;
    apply.started = Instant::now();
}

/// 此回调只由原poll在请求号、修订、World与会话全部匹配的ModeApplied分支调用。
pub(super) fn mode_applied(runtime: &mut Runtime, mode: Mode, revision: u64) {
    if let Some(apply) = runtime.code_apply.as_mut()
        && apply.stage == CodeApplyStage::Restoring
        && apply.session == runtime.identity.as_ref().map(|identity| identity.session)
        && (mode == Mode::Playing) == apply.resume_playing
        && apply.restored_revision.is_some_and(|required| revision >= required)
    {
        apply.mode_confirmed = true;
    }
}

pub(super) fn after_sync(world: &mut World, runtime: &mut Runtime, status: &mut EmbeddedStatus) {
    let Some(apply) = runtime.code_apply.as_ref() else { return; };
    if apply.stage == CodeApplyStage::Restoring && apply.mode_confirmed {
        let token = apply.token;
        runtime.code_apply = None;
        runtime.code_apply_status = Some(CodeApplyStatus { token, state: CodeApplyState::Applied, error: None });
        super::systems::feedback(world, if status.playing {
            "新代码已生效；当前作者草稿已采用并恢复预览，运行态重新开始"
        } else {
            "新代码已生效；当前作者草稿与编辑相机已恢复，作者历史保留"
        });
        return;
    }
    if apply.stage != CodeApplyStage::Synchronizing { return; }
    let capture = world.resource::<EmbeddedSnapshotState>();
    if let Some(error) = capture.error.clone() {
        fail(runtime, &format!("新版本尚未采用当前作者草稿：{error}"));
        if let Some(active) = &runtime.active { active.control.cancel(); }
        return;
    }
    let progress = (capture.revision, runtime.sent_revision, status.applied_revision, capture.pending);
    let apply = runtime.code_apply.as_mut().unwrap();
    if apply.progress != Some(progress) {
        apply.progress = Some(progress);
        apply.started = Instant::now();
    } else if apply.started.elapsed() >= Duration::from_secs(120) {
        let error = "新版本120秒没有草稿采用进展；作者现场保留，可重新应用";
        fail(runtime, error);
        if let Some(active) = &runtime.active { active.control.cancel(); }
        super::systems::feedback(world, error);
        return;
    }
    let empty = capture.map_id.is_none() && !capture.pending
        && runtime.map_id.is_none() && runtime.sent_revision == 0;
    let required = capture.ready.as_ref().map_or(0, |ready| ready.author_revision);
    if !(empty || embedded_snapshot::ready_is_current(world))
        || runtime.snapshot_in_flight.is_some() || status.applied_revision < required
        || !super::previews::settled(world, runtime) || runtime.waiting_mode.is_some()
    { return; }
    let resume_playing = runtime.code_apply.as_ref().unwrap().resume_playing;
    if resume_playing && let Some(reason) = super::systems::play_blocker(world) {
        fail(runtime, &reason);
        super::systems::feedback(world, &reason);
        return;
    }
    let mode = if resume_playing { Mode::Playing } else { Mode::Editing };
    if super::systems::request_mode(runtime, status, mode, required) {
        let apply = runtime.code_apply.as_mut().unwrap();
        apply.stage = CodeApplyStage::Restoring;
        apply.restored_revision = Some(required);
    }
}
