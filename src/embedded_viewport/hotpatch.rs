//! 同实例函数补丁的独立事务门；只传有界元信息，绝不复用会结束旧Job的完整版本应用链。

use super::resources::{EmbeddedStatus, HotpatchFlight, Outgoing, Runtime};
use crate::{code_iteration::hotpatch::{FatBaseline, PatchContext, PatchTarget, PreparedPatch},
    native_play::{CodeApplyState, CodeApplyStatus}, viewport_mode::{ViewportMode, ViewportModeState}};
use bevy::prelude::*;
use feathrise_editor_protocol::Message;
use feathrise_editor_core::{ProjectBuildProfile, ProjectLaunchSettings};
use std::{sync::{Arc, mpsc}, time::{Duration, Instant}};

pub(crate) fn patch_context(world: &World) -> Option<PatchContext> {
    if world.get_resource::<ViewportModeState>()?.selected != ViewportMode::EmbeddedWorld
        || world.get_resource::<ProjectLaunchSettings>().is_some_and(|settings| settings.profile != ProjectBuildProfile::Debug) {
        return None;
    }
    let status = world.get_resource::<EmbeddedStatus>()?;
    let runtime = world.get_non_send::<Runtime>()?;
    if !healthy(runtime, status) || runtime.exit_requested || runtime.close_return_requested || runtime.code_apply.is_some() {
        return None;
    }
    let target = runtime.hotpatch.ready.as_ref()?;
    let baseline = runtime.hotpatch.baseline.as_ref()?;
    if !target_is_current(runtime, status, target) || target.base_build != baseline.base_build { return None; }
    Some(PatchContext { target: target.clone(), baseline: baseline.clone() })
}

/// 点击帧只登记不可变候选；实际管道投递留给drive，避免绕过同帧的新源码、关闭与模式门。
pub(crate) fn request_patch(world: &mut World, prepared: Arc<PreparedPatch>, token: u64) -> Result<(), String> {
    if token == 0 || prepared.source_generation != token { return Err("函数补丁与当前源码代次不一致".into()); }
    if world.get_resource::<crate::native_play::NativePlayStatus>().is_some_and(|status| status.active) {
        return Err("原独立运行尚未结束，请先确认退出".into());
    }
    if let Some(reason) = crate::native_play::code_apply_blocker(world) { return Err(reason); }
    if let Some(reason) = super::systems::play_blocker(world) { return Err(reason); }
    let context = patch_context(world).ok_or("当前工程实例尚未协商可用的函数体热更新基线")?;
    if context.target != prepared.target || context.baseline.base_build != prepared.baseline.base_build {
        return Err("函数补丁属于旧工程实例或旧基线，请保存源码重新构建".into());
    }
    if world.resource::<EmbeddedStatus>().playing { return Err("请先返回编辑，再显式应用函数体热更新".into()); }
    let mut runtime = world.non_send_mut::<Runtime>();
    if runtime.hotpatch.flight.is_some() || runtime.pending_edit || runtime.pending_play || runtime.waiting_mode.is_some()
        || runtime.restart_requested || runtime.snapshot_in_flight.is_some() {
        return Err("当前模式、草稿或代码事务尚未结束，请稍后应用".into());
    }
    runtime.hotpatch.flight = Some(HotpatchFlight { prepared, token, started: Instant::now(), sent: None, timeout_reported: false });
    runtime.code_apply_status = Some(CodeApplyStatus { token, state: CodeApplyState::Applying, error: None });
    Ok(())
}

pub(super) fn cancel_pending(world: &mut World, token: u64, reason: &str) -> bool {
    let Some(mut runtime) = world.get_non_send_mut::<Runtime>() else { return false; };
    if !runtime.hotpatch.flight.as_ref().is_some_and(|flight| flight.token == token && flight.sent.is_none()) { return false; }
    fail(&mut runtime, reason);
    true
}

pub(super) fn baseline(runtime: &mut Runtime, baseline: Arc<FatBaseline>) {
    runtime.hotpatch.baseline = Some(baseline);
    runtime.hotpatch.ready = None;
}

/// 原管道已验证实际进程归属；此处再绑定当前宿主会话、World与冻结EXE摘要。
pub(super) fn ready(runtime: &mut Runtime, status: &EmbeddedStatus, pid: u32, world_id: u64,
    aslr_reference: u64, base_build: String) {
    let Some(identity) = runtime.identity.as_ref() else { return; };
    let Some(baseline) = runtime.hotpatch.baseline.as_ref() else { return; };
    if pid == status.pid && world_id == status.world_id && aslr_reference != 0 && base_build == baseline.base_build {
        runtime.hotpatch.ready = Some(PatchTarget { project_id: identity.project_id.clone(), session: identity.session,
            pid, world_id, aslr_reference, base_build });
    }
}

pub(super) fn advance(world: &mut World, runtime: &mut Runtime, status: &mut EmbeddedStatus,
    selected: ViewportMode, profile: ProjectBuildProfile) {
    let Some(flight) = runtime.hotpatch.flight.as_ref() else { return; };
    if !healthy(runtime, status) || !target_is_current(runtime, status, &flight.prepared.target) {
        fail(runtime, "原工程实例已经结束或身份改变，未把函数补丁标为采用成功");
        return;
    }
    if let Some((_, sent)) = flight.sent {
        if !flight.timeout_reported && sent.elapsed() >= Duration::from_secs(30) {
            runtime.hotpatch.flight.as_mut().unwrap().timeout_reported = true;
            // 已发送的装载可能实际完成，不能清槽重试或冒称已回滚；继续等待可靠回执，关闭仍由原Job收口。
            super::systems::feedback(world, "函数补丁已发送但30秒未确认结果；旧实例仍保留，暂缓后续代码和模式交接，继续等待回执");
        }
        return;
    }
    let invalid = selected != ViewportMode::EmbeddedWorld || profile != ProjectBuildProfile::Debug
        || runtime.exit_requested || runtime.close_return_requested || runtime.code_apply.is_some()
        || runtime.restart_requested || runtime.pending_edit || runtime.pending_play || runtime.waiting_mode.is_some()
        || status.playing || !crate::code_iteration::generation_is_current(world, flight.token)
        || world.get_resource::<crate::native_project_close::NativeProjectCloseState>().is_some_and(|close| close.active());
    if invalid {
        fail(runtime, "源码、工程模式或关闭意图已改变，未发送旧函数补丁");
        return;
    }
    if runtime.snapshot_in_flight.is_some() || !super::previews::settled(world, runtime) {
        if flight.started.elapsed() >= Duration::from_secs(30) {
            fail(runtime, "当前草稿事务持续未结束，函数补丁未发送；原运行实例保留");
        }
        return;
    }
    let generation = runtime.identity.as_ref().unwrap().generation;
    let command = Outgoing { generation, payload: None, message: Message::ApplyCodePatch {
        patch_id: flight.token, source_generation: flight.prepared.source_generation,
        base_build: flight.prepared.target.base_build.clone(), artifact: flight.prepared.artifact.clone(), sha256: flight.prepared.sha256.clone(),
    }};
    match runtime.active.as_ref().unwrap().sender.try_send(command) {
        Ok(()) => {
            runtime.hotpatch.flight.as_mut().unwrap().sent = Some((generation, Instant::now()));
            super::systems::feedback(world, "正在原工程实例中应用函数体热更新，场景与运行状态保留");
        }
        Err(mpsc::TrySendError::Full(_)) => {
            if runtime.hotpatch.flight.as_ref().unwrap().started.elapsed() >= Duration::from_secs(30) {
                fail(runtime, "控制队列持续繁忙，函数补丁未发送；原运行实例保留");
            }
        }
        Err(mpsc::TrySendError::Disconnected(_)) => fail(runtime, "控制工作槽已结束，函数补丁未发送"),
    }
}

pub(super) fn result(world: &mut World, runtime: &mut Runtime, status: &mut EmbeddedStatus,
    patch_id: u64, source_generation: u64, base_build: String, pid: u32, world_id: u64,
    epoch: u64, applied: bool, reason: Option<String>) {
    let Some(flight) = runtime.hotpatch.flight.as_ref() else { return; };
    let Some((generation, _)) = flight.sent else { return; };
    let target = &flight.prepared.target;
    if patch_id != flight.token || source_generation != flight.prepared.source_generation || base_build != target.base_build
        || pid != target.pid || world_id != target.world_id || !target_is_current(runtime, status, target)
        || runtime.identity.as_ref().is_none_or(|identity| identity.generation != generation) { return; }
    let expected = if applied { status.patch_epoch.checked_add(1) } else { Some(status.patch_epoch) };
    if expected != Some(epoch) {
        if !flight.timeout_reported {
            runtime.hotpatch.flight.as_mut().unwrap().timeout_reported = true;
            super::systems::feedback(world, "函数补丁回执的采用代次不匹配，保留原实例并阻止继续补丁，等待关闭后重新连接");
        }
        return;
    }
    let token = flight.token;
    runtime.hotpatch.flight = None;
    if applied {
        status.patch_epoch = epoch;
        runtime.code_apply_status = Some(CodeApplyStatus { token, state: CodeApplyState::Applied, error: None });
        super::systems::feedback(world, "函数体热更新已生效；工程进程、场景和运行状态保持连续");
    } else {
        runtime.code_apply_status = Some(CodeApplyStatus { token, state: CodeApplyState::Failed,
            error: Some(reason.unwrap_or_else(|| "运行实例拒绝函数补丁；原有效版本保留".into())) });
    }
}

pub(super) fn reset(runtime: &mut Runtime, reason: &str) {
    fail(runtime, reason);
    runtime.hotpatch = default();
}

fn fail(runtime: &mut Runtime, reason: &str) {
    if let Some(flight) = runtime.hotpatch.flight.take() {
        runtime.code_apply_status = Some(CodeApplyStatus { token: flight.token, state: CodeApplyState::Failed, error: Some(reason.into()) });
    }
}

fn target_is_current(runtime: &Runtime, status: &EmbeddedStatus, target: &PatchTarget) -> bool {
    runtime.identity.as_ref().is_some_and(|identity| identity.project_id == target.project_id && identity.session == target.session)
        && status.pid == target.pid && status.world_id == target.world_id
}

fn healthy(runtime: &Runtime, status: &EmbeddedStatus) -> bool {
    status.active && status.ready && status.error.is_none() && runtime.cleanup_failure.is_none()
        && runtime.active.as_ref().is_some_and(|active| !active.control.is_cancelled())
}

#[cfg(test)]
#[path = "hotpatch_tests.rs"]
mod tests;
