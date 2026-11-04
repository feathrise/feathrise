//! 编译完成值与明确采用意图的主线程协调；不读取源码、不等待后台工作或丢弃作者World。

use super::{Event, resources::*, types::CodeIterationPhase as Phase, RequestApplyCode};
use crate::{native_play::CodeApplyState, viewport_mode::{ViewportMode, ViewportModeState}};
use bevy::{ecs::message::MessageCursor, prelude::*, window::WindowCloseRequested};
use feathrise_editor_core::ProjectLaunchSettings;
use std::{sync::{Arc, mpsc::TryRecvError}};

pub(super) fn sync_patch_context(world: &mut World) {
    let context = crate::embedded_viewport::patch_context(world);
    if let Some(backend) = world.non_send_mut::<CodeIterationRuntime>().backend.as_mut() {
        backend.set_patch_context(context);
    }
}

pub(super) fn poll(
    launch: Res<ProjectLaunchSettings>,
    mut state: ResMut<super::CodeIterationState>,
    mut runtime: NonSendMut<CodeIterationRuntime>,
) {
    // 稳定运行不借用整个作者状态的可变引用，避免每帧误标changed并触发代码UI重排。
    if runtime.lifecycle != BackendLifecycle::Running
        && !super::lifecycle::advance(&mut runtime, &mut state, launch.profile) { return; }
    if runtime.profile != launch.profile {
        if let Some(backend) = runtime.backend.as_ref() { backend.set_profile(launch.profile); }
        runtime.profile = launch.profile;
        state.pending = None;
        state.phase = Phase::Waiting;
        state.status = format!("代码：等待{}配置的最新版本", launch.profile.label());
    }
    let Some(backend) = runtime.backend.as_ref() else { return; };
    for _ in 0..8 { // 完成邮箱与每帧消费均固定预算；完整诊断不在帧上解析。
        let event = match backend.try_recv() {
            Ok(event) => event,
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => {
                if state.phase != Phase::Failed {
                    state.phase = Phase::Failed;
                    state.status = "代码：自动编译服务已停止，请重新打开工程；当前工程实例保留".into();
                }
                state.pending = None;
                break;
            }
        };
        let current = match &event {
            Event::Building { identity } | Event::Finished { identity, .. } => backend.is_current(identity),
            Event::Waiting { generation } => *generation == backend.current_generation(),
            Event::Failed(_) => true,
        };
        adopt_event(&mut state, event, current);
    }
}

/// 身份失效时零状态副作用；旧编译成功、旧错误与旧配置都不能替换最新代码状态。
pub(super) fn adopt_event(state: &mut super::CodeIterationState, event: Event, current: bool) {
    if !current { return; }
    match event {
        Event::Waiting { generation } => {
            state.generation = generation;
            state.pending = None;
            state.phase = Phase::Waiting;
            state.status = "代码：源码已保存，等待后台编译".into();
            clear_diagnostics(state);
        }
        Event::Building { identity } if identity.project_id == state.project_id && identity.root == state.project_root => {
            state.generation = identity.generation;
            state.pending = None;
            state.phase = Phase::Building;
            state.status = format!("代码：正在后台编译（{}），当前工程实例继续运行", identity.profile.label());
            clear_diagnostics(state);
        }
        Event::Finished { identity, result, diagnostics }
            if identity.project_id == state.project_id && identity.root == state.project_root => {
            state.generation = identity.generation;
            state.diagnostics = Arc::new(diagnostics);
            state.diagnostic_revision = state.diagnostic_revision.wrapping_add(1);
            match result {
                Ok(version) if version.identity == identity => {
                    let patch = matches!(version.artifact, super::PreparedArtifact::Patch(_));
                    state.pending = Some(version);
                    state.phase = Phase::Ready;
                    state.status = if patch { "代码：函数修改已编译，可原地应用热更新" } else { "代码：编译成功，可应用并重启预览" }.into();
                }
                Ok(_) => {
                    state.pending = None;
                    state.phase = Phase::Failed;
                    state.status = "代码：构建版本身份不一致，本次未采用".into();
                }
                Err(error) => {
                    state.pending = None;
                    state.phase = Phase::Failed;
                    state.status = format!("代码：编译失败，点击“代码”查看诊断；{error}");
                }
            }
        }
        Event::Failed(error) => {
            state.pending = None;
            state.phase = Phase::Failed;
            state.status = format!("代码：{error}；当前工程实例与作者草稿保留");
        }
        _ => (),
    }
}

pub(super) fn clear_diagnostics(state: &mut super::CodeIterationState) {
    if !state.diagnostics.is_empty() {
        state.diagnostics = Arc::new(Vec::new());
        state.diagnostic_revision = state.diagnostic_revision.wrapping_add(1);
    }
}

/// 保存更新源码或切配置后，尚未真正启动的旧应用意图作废；已经启动的版本继续作为旧运行版本。
pub(super) fn guard_application(world: &mut World) {
    let Some((token, mode, valid)) = ({
        let state = world.resource::<super::CodeIterationState>();
        state.applying.as_ref().map(|flight| {
            let current = world.non_send::<CodeIterationRuntime>().backend.as_ref()
                .is_some_and(|backend| backend.is_current(&flight.version.identity))
                && world.resource::<ProjectLaunchSettings>().profile == flight.version.identity.profile
                && world.resource::<ViewportModeState>().selected == flight.mode;
            (flight.token, flight.mode, current)
        })
    }) else { return; };
    if valid { return; }
    let reason = "已有更新源码或构建配置，尚未启动的旧代码应用已撤销；作者现场保留";
    match mode {
        ViewportMode::EmbeddedWorld => { crate::embedded_viewport::cancel_pending_code_apply(world, token, reason); }
        ViewportMode::SavedWorld => { crate::native_play::cancel_pending_code_apply(world, token, reason); }
    }
}

pub(super) fn apply(
    world: &mut World, mut requests: Local<MessageCursor<RequestApplyCode>>,
    mut closes: Local<MessageCursor<WindowCloseRequested>>,
) {
    // 关闭消息可在原窗口队列中保留；每帧推进自己的游标，只阻断新请求。
    // 进行中的确认由原关闭状态接管，取消后的旧消息不能永久吞掉后续应用按钮。
    let closing_now = world.get_resource::<Messages<WindowCloseRequested>>()
        .is_some_and(|messages| closes.read(messages).last().is_some());
    let Some(request) = requests.read(world.resource::<Messages<RequestApplyCode>>()).last().copied() else { return; };
    let blocked = world.get_resource::<crate::native_project_close::NativeProjectCloseState>().is_some_and(|close| close.captures_input())
        || world.get_resource::<feathrise_editor_core::EditorHostOverlay>().is_some_and(|overlay| overlay.active)
        || closing_now;
    let state = world.resource::<super::CodeIterationState>();
    if blocked || state.diagnostics_open || state.applying.is_some() { return; }
    let Some(version) = state.pending.as_ref().filter(|version| version.identity.generation == request.generation).cloned() else { return; };
    let current = world.non_send::<CodeIterationRuntime>().backend.as_ref().is_some_and(|backend| backend.is_current(&version.identity));
    let profile = world.resource::<ProjectLaunchSettings>().profile;
    if !current || version.identity.profile != profile {
        let mut state = world.resource_mut::<super::CodeIterationState>();
        state.pending = None;
        state.phase = Phase::Waiting;
        state.status = "代码：已有更新的源码或配置，等待最新版本编译".into();
        return;
    }
    let mode = world.resource::<ViewportModeState>().selected;
    let patch = matches!(version.artifact, super::PreparedArtifact::Patch(_));
    let result = match (&version.artifact, mode) {
        (super::PreparedArtifact::Patch(patch), ViewportMode::EmbeddedWorld) => crate::embedded_viewport::request_patch(world, patch.clone(), request.generation),
        (super::PreparedArtifact::Patch(_), _) => Err("该热更新候选属于工程3D的原运行实例，请切回后应用或重新保存编译".into()),
        (super::PreparedArtifact::Restart(game), ViewportMode::EmbeddedWorld) => crate::embedded_viewport::request_code_apply(world, game.clone(), request.generation),
        (super::PreparedArtifact::Restart(game), ViewportMode::SavedWorld) => crate::native_play::request_code_apply(world, game.clone(), request.generation),
    };
    let mut state = world.resource_mut::<super::CodeIterationState>();
    match result {
        Ok(()) => {
            state.applying = Some(ApplyFlight { token: request.generation, mode, version });
            state.phase = Phase::Applying;
            state.status = if patch { "代码：正在原运行实例应用函数热更新，运行状态和作者现场保留" }
                else { "代码：正在收口原工程实例并应用新版本，作者现场保留" }.into();
        }
        Err(error) => {
            state.status = format!("代码：暂未应用；{error}"); // 候选继续保留，可在原事务完成后再次明确点击。
        }
    }
}

pub(super) fn application_result(world: &mut World) {
    let Some((token, mode)) = world.resource::<super::CodeIterationState>().applying.as_ref().map(|flight| (flight.token, flight.mode)) else { return; };
    let result = match mode {
        ViewportMode::EmbeddedWorld => crate::embedded_viewport::code_apply_status(world),
        ViewportMode::SavedWorld => crate::native_play::code_apply_status(world),
    };
    let Some(result) = result.filter(|result| result.token == token && result.state != CodeApplyState::Applying) else { return; };
    let current = {
        let state = world.resource::<super::CodeIterationState>();
        let identity = &state.applying.as_ref().unwrap().version.identity;
        world.non_send::<CodeIterationRuntime>().backend.as_ref().is_some_and(|backend| backend.is_current(identity))
    };
    let mut state = world.resource_mut::<super::CodeIterationState>();
    let Some(flight) = state.applying.take() else { return; };
    match result.state {
        CodeApplyState::Applied => {
            state.applied = Some(flight.version.identity.clone());
            if state.pending.as_ref().is_some_and(|candidate| candidate.identity == flight.version.identity) { state.pending = None; }
            if current {
                state.phase = Phase::Applied;
                state.status = if matches!(flight.version.artifact, super::PreparedArtifact::Patch(_)) { "代码：函数热更新已生效，运行状态和作者现场保留" }
                    else if mode == ViewportMode::EmbeddedWorld { "代码：新版本已生效，草稿与编辑现场已恢复" }
                    else { "代码：新版本工程实例已启动，作者草稿和编辑现场保留" }.into();
            } // 应用期间又保存了源码时，仍保留新源码的编译/失败状态，不把较早版本说成最新。
        }
        CodeApplyState::Failed => {
            if current || state.phase == Phase::Applying {
                state.phase = Phase::Failed;
                state.status = format!("代码：应用失败，作者现场保留；{}", result.error.as_deref().unwrap_or("请查看工程日志"));
            }
        }
        CodeApplyState::Applying => (),
    }
}
