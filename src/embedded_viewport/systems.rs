//! 帧内协调只交换有限消息和Arc；构建、管道、摘要与快照编码由各自后台槽完成。

use super::resources::Event;
use super::resources::*;
use crate::viewport_mode::{
    ViewportGeometry, ViewportMode, ViewportModeAvailability, ViewportModeState,
};
use bevy::{ecs::message::MessageCursor, prelude::*};
use feathrise_editor_protocol::{CameraState, Message, Mode, SessionIdentity};
use feathrise_editor_map::standalone::{
    EmbeddedAuthorInput, RunSavedProject, SavedPlayFeedback, StandaloneEditorMode,
    embedded_snapshot::{self, EmbeddedSnapshotState},
};
use std::{
    sync::{Arc, mpsc},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub(super) fn drive(world: &mut World, mut requests: Local<MessageCursor<RunSavedProject>>) {
    let selected = world.resource::<ViewportModeState>().selected;
    let selection_revision = world.resource::<ViewportModeState>().revision;
    let profile = world.get_resource::<feathrise_editor_core::ProjectLaunchSettings>()
        .map_or(default(), |settings| settings.profile);
    let build_backend = crate::native_play::ProjectBuildBackend::selected(world);
    let geometry = world.resource::<ViewportGeometry>().clone();
    let request = requests
        .read(world.resource::<Messages<RunSavedProject>>())
        .last()
        .cloned();
    // 发布时冻结的配置与当前选择不同，说明同帧发生了显式更换；旧意图不能启动另一种产物。
    let requested = request.is_some_and(|request| request.profile == profile);
    let mut runtime = world
        .remove_non_send::<Runtime>()
        .expect("常驻会话资源已安装");
    // 停止期间的旧预览点击只丢弃，不得在本帧排空后变成一次重启；排空后的明确点击仍可重试。
    let requested = requested && !runtime.active.as_ref().is_some_and(|active| active.control.is_cancelled());
    let mut status = world
        .remove_resource::<EmbeddedStatus>()
        .expect("常驻状态已安装");
    world.resource_mut::<EmbeddedSnapshotState>().enabled = selected == ViewportMode::EmbeddedWorld;

    poll(world, &mut runtime, &mut status);
    super::hotpatch::advance(world, &mut runtime, &mut status, selected, profile);
    if runtime.attempted_profile.is_some_and(|attempted| attempted != profile)
        || runtime.attempted_backend.is_some_and(|attempted| attempted != build_backend) {
        runtime.pending_play = false;
        runtime.pending_edit = false;
        runtime.restart_requested = false;
        runtime.play_wait = None;
        if let Some(active) = &runtime.active { active.control.cancel(); }
    }
    if runtime.close_return_requested {
        runtime.pending_play = false;
        if status.playing {
            runtime.pending_edit = true;
        }
    }
    // 返回编辑不加载场景，不应沿用首次预览的长准备预算；断链时及时恢复可见的关闭确认。
    let mode_budget = if runtime
        .waiting_mode
        .is_some_and(|(_, mode)| mode == Mode::Editing)
    {
        Duration::from_secs(5)
    } else {
        Duration::from_secs(75)
    };
    if runtime
        .mode_started
        .is_some_and(|started| started.elapsed() > mode_budget)
        || runtime
            .snapshot_started
            .is_some_and(|started| started.elapsed() > Duration::from_secs(120))
    {
        if let Some(active) = &runtime.active {
            active.control.cancel();
        }
        runtime.mode_started = None;
        runtime.snapshot_started = None;
        status.error = Some("运行实例未在事务预算内确认当前草稿或模式，正在收口；作者现场保留".into());
        feedback(world, status.error.as_deref().unwrap());
    }
    enforce_close_return_budget(world, &mut runtime, &mut status, Instant::now());
    super::code_apply::before_start(world, &mut runtime, &mut status, selected, profile);
    if selected != ViewportMode::EmbeddedWorld || runtime.exit_requested {
        runtime.restart_requested = false;
        if let Some(active) = &runtime.active {
            active.control.cancel();
        }
    } else if runtime.active.is_none()
        && runtime.cleanup_failure.is_none()
        && !world
            .resource::<crate::native_play::NativePlayStatus>()
            .active
        && geometry.parent_hwnd != 0
        && (runtime.code_apply.is_none() || super::code_apply::ready_to_start(&runtime))
        && ((runtime.compilation_stop_input != Some((selection_revision, profile))
                && (selection_revision != runtime.attempted_selection
                    || runtime.attempted_profile != Some(profile)
                    || runtime.attempted_backend.is_some_and(|attempted| attempted != build_backend)))
            || requested
            || runtime.restart_requested
            || super::code_apply::ready_to_start(&runtime))
    {
        let retry_play = runtime.restart_requested;
        let prepared = runtime.code_apply.as_ref().map(|apply| apply.prepared.clone());
        start(
            world,
            &mut runtime,
            &mut status,
            geometry.clone(),
            selection_revision,
            profile,
            build_backend,
            prepared,
        );
        runtime.restart_requested = false;
        if runtime.code_apply.is_some() {
            super::code_apply::started(&mut runtime);
        } else {
            runtime.pending_play = retry_play;
        }
    }

    if selected == ViewportMode::EmbeddedWorld
        && requested
        && !runtime.close_return_requested
        && runtime.active.is_some()
        && !runtime.restart_requested
        && runtime.code_apply.is_none()
        && runtime.hotpatch.flight.is_none()
        && runtime.attempted_profile.is_none_or(|attempted| attempted == profile)
        && runtime.attempted_backend.is_none_or(|attempted| attempted == build_backend)
    {
        // 顶部预览按钮复用既有可靠模式交接，stop_only仍沿用旧消息合同。
        if status.playing {
            runtime.pending_edit = true;
        } else if status.error.is_some() && !runtime.restart_requested {
            // 错误快照不自动无限重试；预览按钮明确重连，新会话重新提交同一份完整作者草稿。
            runtime.restart_requested = true;
            runtime.pending_play = false;
            runtime
                .active
                .as_ref()
                .expect("活动会话已检查")
                .control
                .cancel();
            feedback(
                world,
                "正在收口失败会话，随后重新连接并采用当前草稿；作者历史保留",
            );
        } else if runtime.waiting_mode.is_none() {
            if !runtime.pending_play
                && let Some(reason) = play_blocker(world)
            {
                feedback(world, &reason);
            } else {
                runtime.pending_play = !runtime.pending_play;
                feedback(
                    world,
                    if runtime.pending_play {
                        "等待当前草稿完整采用后进入预览；再次点击“预览”取消等待"
                    } else {
                        "已取消等待预览，作者现场保留"
                    },
                );
            }
        }
    }

    let stopping = runtime
        .active
        .as_ref()
        .is_some_and(|active| active.control.is_cancelled());
    if runtime.active.is_some()
        && status.ready
        && !runtime.exit_requested
        && !stopping
        && selected == ViewportMode::EmbeddedWorld
    {
        // 返回编辑是可靠意图；邮箱满时留在唯一槽重试，不能要求再次点击游戏工具条。
        if runtime.pending_edit && request_mode(&mut runtime, &mut status, Mode::Editing, 0) {
            runtime.pending_edit = false;
        }
        sync_view(world, &mut runtime, &mut status, geometry);
        sync_snapshot(world, &mut runtime, &mut status);
        super::previews::sync(world, &mut runtime, &mut status);
        sync_overlay(world, &mut runtime, &mut status);
        super::tools::synchronize(world, &mut runtime, &mut status);
        super::code_apply::after_sync(world, &mut runtime, &mut status);
        if runtime.pending_play && runtime.waiting_mode.is_none() {
            let capture = world.resource::<EmbeddedSnapshotState>();
            let ready = embedded_snapshot::ready_is_current(world);
            let author_empty =
                capture.map_id.is_none() && !capture.pending && capture.error.is_none();
            let empty = author_empty && runtime.map_id.is_none() && runtime.sent_revision == 0;
            let required = capture
                .ready
                .as_ref()
                .map_or(0, |snapshot| snapshot.author_revision);
            let error = capture.error.clone();
            if author_empty && !empty {
                // 关闭作者图并不是清空游戏World的事务，不能把上一地图冒充新空工程预览。
                runtime.pending_play = false;
                feedback(
                    world,
                    "当前作者地图已关闭；请打开要预览的地图，或重新连接工程视口后预览空工程",
                );
            } else if let Some(error) = error {
                runtime.pending_play = false;
                feedback(
                    world,
                    &format!("当前草稿尚不能预览：{error}；已保留作者现场"),
                );
            } else if (ready || empty)
                && status.applied_revision >= required
                && super::previews::settled(world, &runtime)
            {
                if let Some(reason) = play_blocker(world) {
                    runtime.pending_play = false;
                    feedback(world, &reason);
                } else if request_mode(&mut runtime, &mut status, Mode::Playing, required) {
                    runtime.pending_play = false;
                }
            }
        }
    }
    super::previews::configure(world, &mut runtime, &status, selected);
    // 仅模式交接期间冻结作者输入。稳定Playing与独立启动一样允许操作作者工作台；
    // sync_snapshot/实时预览仍拒绝Playing写入，后续草稿在返回Editing后再完整采用。
    let close_owns_input = world
        .get_resource::<crate::native_project_close::NativeProjectCloseState>()
        .is_some_and(|state| state.active());
    // 工程关闭层已有原作者隔离门；让它继续接收保存/取消选择，避免锁住关闭确认。
    world.resource_mut::<EmbeddedAuthorInput>().set_blocked(
        runtime.active.is_some()
            && runtime.waiting_mode.is_some()
            && !close_owns_input,
    );
    // 按钮状态只跟真实模式回执变化，发送请求不冒充已经进入游戏。
    if let Some(mut editor) = world.get_resource_mut::<StandaloneEditorMode>() {
        if selected == ViewportMode::EmbeddedWorld || runtime.active.is_some() {
            editor.set_external_play_active(status.playing);
        }
    }
    // 模式事实只更新运行按钮，不改变原工作台的显隐、布局、作者相机和文档焦点。
    feathrise_editor_map::standalone::set_embedded_playing(world, status.playing);
    world
        .resource_mut::<ViewportModeAvailability>()
        .blocked_reason = if runtime.cleanup_failure.is_some() {
        runtime.cleanup_failure.clone()
    } else if runtime.code_apply.is_some() || runtime.hotpatch.flight.is_some() {
        Some("正在应用新代码并恢复当前草稿与原模式，请稍候再切换".into())
    } else if stopping {
        Some("正在确认当前运行进程和通信线程退出，请稍候再切换".into())
    } else if runtime.active.is_some()
        && (!status.ready || status.playing || runtime.waiting_mode.is_some())
    {
        Some("请先完成构建或返回编辑，再切换3D方式".into())
    } else {
        None
    };
    present_capture_diagnostic(world, &mut runtime, &status, selected);
    present_play_wait(world, &mut runtime, &status, selected, Instant::now());
    if let Some(mut launch) = world.get_resource_mut::<feathrise_editor_core::ProjectLaunchSettings>() {
        launch.switching_profile = runtime.active.is_some() && !runtime.compilation_stopped
            && (runtime.attempted_profile.is_some_and(|attempted| attempted != profile)
                || runtime.attempted_backend.is_some_and(|attempted| attempted != build_backend));
    }
    world.insert_resource(status);
    world.insert_non_send(runtime);
}

/// 关闭是独立可靠意图。控制邮箱一直满时尚无mode_started，持续心跳也不能延长此预算。
fn enforce_close_return_budget(
    world: &mut World,
    runtime: &mut Runtime,
    status: &mut EmbeddedStatus,
    now: Instant,
) {
    if !runtime.close_return_requested || (!status.playing && runtime.waiting_mode.is_none()) {
        runtime.close_return_started = None;
        return;
    }
    let Some(started) = runtime.close_return_started else {
        return;
    };
    if now.saturating_duration_since(started) < Duration::from_secs(5) {
        return;
    }
    let Some(active) = runtime.active.as_ref() else {
        return;
    };
    if active.control.is_cancelled() {
        return;
    }
    // 仅通知既有拥有者停止专属Job，仍等待poll核实线程/进程收口后才恢复作者确认层。
    active.control.cancel();
    runtime.pending_play = false;
    runtime.pending_edit = false;
    runtime.restart_requested = false;
    runtime.mode_started = None;
    if status.error.is_none() {
        let message = "关闭等待返回编辑超过5秒，正在结束本次运行实例；作者草稿和历史保留，收口后继续原关闭确认";
        status.error = Some(message.into());
        feedback(world, message);
    }
}

#[derive(Debug, PartialEq, Eq)]
enum WaitObservation {
    Changed,
    Unchanged,
    TimedOut,
}

fn observe_play_wait(
    wait: &mut Option<PlayWait>,
    progress: PlayWaitProgress,
    connected: bool,
    now: Instant,
) -> WaitObservation {
    if wait
        .as_ref()
        .is_none_or(|wait| wait.progress != progress || wait.last_progress.is_some() != connected)
    {
        *wait = Some(PlayWait {
            progress,
            last_progress: connected.then_some(now),
        });
        return WaitObservation::Changed;
    }
    if wait
        .as_ref()
        .and_then(|wait| wait.last_progress)
        .is_some_and(|started| now.duration_since(started) >= Duration::from_secs(120))
    {
        *wait = None;
        WaitObservation::TimedOut
    } else {
        WaitObservation::Unchanged
    }
}

fn present_play_wait(
    world: &mut World,
    runtime: &mut Runtime,
    status: &EmbeddedStatus,
    selected: ViewportMode,
    now: Instant,
) {
    let capture = world.resource::<EmbeddedSnapshotState>();
    let waiting_mode = runtime
        .waiting_mode
        .is_some_and(|(_, mode)| mode == Mode::Playing);
    if selected != ViewportMode::EmbeddedWorld
        || status.playing
        || runtime.exit_requested
        || runtime
            .active
            .as_ref()
            .is_none_or(|active| active.control.is_cancelled())
        || (!runtime.pending_play && !waiting_mode)
        || status.error.is_some()
        || runtime.cleanup_failure.is_some()
        || capture.error.is_some()
    {
        runtime.play_wait = None;
        return;
    }
    let empty = capture.map_id.is_none()
        && !capture.pending
        && runtime.map_id.is_none()
        && runtime.sent_revision == 0;
    let ready_revision = capture
        .ready
        .as_ref()
        .map_or(0, |value| value.author_revision);
    let phase = if !status.ready {
        PlayWaitPhase::Connecting
    } else if waiting_mode {
        PlayWaitPhase::Readiness
    } else if !empty && !embedded_snapshot::ready_is_current(world) {
        PlayWaitPhase::Preparing
    } else if runtime.snapshot_in_flight.is_some() || status.applied_revision < ready_revision {
        PlayWaitPhase::Applying
    } else {
        PlayWaitPhase::Readiness
    };
    let progress = PlayWaitProgress {
        phase,
        reason: (phase == PlayWaitPhase::Preparing)
            .then(|| embedded_snapshot::waiting_reason(world))
            .flatten(),
        // 构建阶段只有连接本身才算进展，不因作者每次编辑重复刷构建提示。
        author_revision: if status.ready { capture.revision } else { 0 },
        ready_revision: if status.ready { ready_revision } else { 0 },
        capture_pending: status.ready && capture.pending,
        sent_revision: if status.ready {
            runtime.sent_revision
        } else {
            0
        },
        applied_revision: if status.ready {
            status.applied_revision
        } else {
            0
        },
    };
    let observation =
        observe_play_wait(&mut runtime.play_wait, progress.clone(), status.ready, now);
    if observation == WaitObservation::Unchanged {
        return;
    }
    let detail = match phase {
        PlayWaitPhase::Connecting => "等待运行实例完成构建和连接；首次构建可能较久".into(),
        PlayWaitPhase::Preparing => format!(
            "准备草稿（修订{}）：{}",
            progress.author_revision,
            progress.reason.unwrap_or("准备当前完整作者来源")
        ),
        PlayWaitPhase::Applying => format!("等待运行实例采用草稿（修订{}）", progress.ready_revision),
        PlayWaitPhase::Readiness => format!(
            "等待工程场景、资产和碰撞就绪（修订{}）",
            progress.ready_revision
        ),
    };
    match observation {
        WaitObservation::Changed => feedback(world, &detail),
        WaitObservation::Unchanged => (),
        WaitObservation::TimedOut => {
            // 仅取消本次预览意图；不伪造进程失败、不删除作者现场，也不重启健康游戏。
            // 已发送Snapshot和SetMode继续服从原120/75秒事务预算。
            runtime.pending_play = false;
            feedback(
                world,
                &format!(
                    "预览等待停滞120秒（{detail}），已取消本次等待；作者草稿和历史保留，点击“预览”可重试"
                ),
            );
        }
    }
}

fn present_capture_diagnostic(
    world: &mut World,
    runtime: &mut Runtime,
    status: &EmbeddedStatus,
    selected: ViewportMode,
) {
    if selected != ViewportMode::EmbeddedWorld || status.playing || runtime.compilation_stopped {
        return;
    }
    let error = world.resource::<EmbeddedSnapshotState>().error.clone();
    let cleared = runtime.capture_error.is_some() && error.is_none();
    runtime.capture_error = error;
    // 来源拒绝和进程故障分开；真实故障已有诊断及重连路径，不能被捕获状态覆盖。
    if status.error.is_some() || runtime.cleanup_failure.is_some() {
        return;
    }
    if let Some(error) = &runtime.capture_error {
        let message = format!("当前草稿未同步到工程实例：{error}；上一有效画面和作者草稿保留");
        // 放在本帧协调末尾，防止迟到的 Ready/Progress 把持续错误冒充“正在采用”。
        // 仅当可见文字被新事件改变时恢复一次，不在静止帧重复发布反馈消息。
        if world.resource::<ViewportModeState>().status != message {
            feedback(world, &message);
        }
    } else if cleared {
        feedback(world, "作者草稿已更新，正在重新准备工程视口快照");
    }
}

fn start(
    world: &mut World,
    runtime: &mut Runtime,
    status: &mut EmbeddedStatus,
    geometry: ViewportGeometry,
    selection: u64,
    profile: feathrise_editor_core::ProjectBuildProfile,
    build_backend: crate::native_play::ProjectBuildBackend,
    prepared: Option<Arc<crate::native_play::PreparedGame>>,
) {
    let settings = world.resource::<Settings>();
    let project = settings.project.clone();
    let target = settings.target.clone();
    let session = (SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64)
        .max(1);
    let identity = SessionIdentity {
        project_id: project.id.clone(),
        session,
        generation: 1,
    };
    let worker_identity = identity.clone();
    let control = world.get_resource::<crate::project_log::ProjectLogChannel>()
        .map(|logs| crate::build_process::BuildControl::with_log(logs.begin_task(crate::project_log::LogTaskKind::Project3d)))
        .unwrap_or_default();
    let worker_control = control.clone();
    let (sender, commands) = mpsc::sync_channel(4);
    let (events, receiver) = mpsc::sync_channel(16);
    let (result_sender, result) = mpsc::sync_channel(1);
    let handle = thread::spawn(move || {
        #[cfg(windows)]
        let result = super::worker::run(
            project,
            target,
            profile,
            build_backend,
            prepared,
            geometry,
            worker_identity,
            worker_control,
            commands,
            events,
        );
        #[cfg(not(windows))]
        let result: Result<(), String> = {
            // UI和偏好均已屏蔽此能力；异常外部请求也明确失败，绝不伪造工程呈现回执。
            drop((project, target, profile, build_backend, prepared, geometry, worker_identity, worker_control, commands, events));
            Err("工程 3D 需要 Windows 窗口支持。请切换到编辑器 3D，再点击“启动”".into())
        };
        let _ = result_sender.send(result);
    });
    runtime.active = Some(Active {
        control,
        sender,
        receiver,
        result,
        thread: handle,
    });
    super::hotpatch::reset(runtime, "工程实例已重新启动，旧函数补丁身份失效");
    runtime.identity = Some(identity);
    runtime.attempted_selection = selection;
    runtime.attempted_profile = Some(profile);
    runtime.attempted_backend = Some(build_backend);
    runtime.compilation_stopped = false;
    runtime.compilation_stop_input = None;
    runtime.waiting_mode = None;
    runtime.pending_play = false;
    runtime.play_wait = None;
    runtime.pending_edit = false;
    runtime.close_return_started = None;
    runtime.snapshot_in_flight = None;
    runtime.snapshot_started = None;
    runtime.capture_error = None;
    runtime.mode_started = None;
    runtime.sent_revision = 0;
    runtime.geometry_revision = 0;
    runtime.camera = None;
    runtime.author_view = None;
    runtime.map_id = None;
    runtime.overlay_generation = 0;
    runtime.tools_generation = 0;
    status.active = true;
    status.ready = false;
    status.playing = false;
    status.error = None;
    status.applied_revision = 0;
    status.frames = 0;
    status.ticks = 0;
    status.observed_mode = None;
    status.pid = 0;
    status.world_id = 0;
    status.patch_epoch = 0;
    status.starts = status.starts.saturating_add(1);
    feedback(
        world,
        &format!("正在后台构建{}并准备常驻工程视口；作者文档保持可用", profile.label()),
    );
}

fn poll(world: &mut World, runtime: &mut Runtime, status: &mut EmbeddedStatus) {
    let events: Vec<_> = runtime
        .active
        .as_ref()
        .map(|active| active.receiver.try_iter().take(16).collect())
        .unwrap_or_default();
    for event in events {
        // 取消撤回的是整个旧会话的采用权；仍有界排空邮箱，但迟到进度与协议均不能复活旧启动。
        if runtime.active.as_ref().is_some_and(|active| active.control.is_cancelled()) {
            continue;
        }
        match event {
            Event::Progress(message) => feedback(world, &message),
            Event::HotpatchBaseline(baseline) => super::hotpatch::baseline(runtime, baseline),
            Event::Protocol(frame) => {
                if runtime
                    .identity
                    .as_ref()
                    .is_none_or(|identity| *identity != frame.identity)
                {
                    continue;
                }
                match frame.message {
                    Message::Ready { pid, world_id, .. } => {
                        status.ready = true;
                        status.pid = pid;
                        status.world_id = world_id;
                        feedback(world, "工程视口已连接；正在采用当前作者草稿");
                    }
                    Message::HotPatchReady { pid, world_id, aslr_reference, base_build } => {
                        super::hotpatch::ready(runtime, status, pid, world_id, aslr_reference, base_build);
                    }
                    Message::CodePatchResult { patch_id, source_generation, base_build, pid, world_id, epoch, applied, reason } => {
                        super::hotpatch::result(world, runtime, status, patch_id, source_generation, base_build, pid, world_id, epoch, applied, reason);
                    }
                    Message::SnapshotApplied {
                        transaction,
                        revision,
                    } => {
                        if runtime.snapshot_in_flight == Some(transaction)
                            && revision == transaction
                        {
                            feathrise_editor_map::standalone::embedded_author_preview::snapshot_applied(
                                world, frame.identity.session, frame.identity.generation, revision,
                            );
                            runtime.snapshot_in_flight = None;
                            runtime.snapshot_started = None;
                            status.applied_revision = status.applied_revision.max(revision);
                            // 工具值可能未变化，但场景采用后必须重绑新的正式修订。
                            runtime.tools_generation = 0;
                        }
                    }
                    Message::PreviewApplied { kind, sequence } => {
                        super::previews::acknowledge(world, runtime, kind, sequence);
                    }
                    Message::ModeApplied {
                        request_id,
                        mode,
                        revision,
                        world_id,
                    } => {
                        if runtime.waiting_mode == Some((request_id, mode))
                            && status.world_id == world_id
                            && revision >= runtime.waiting_revision
                            && revision <= runtime.sent_revision
                        {
                            runtime.waiting_mode = None;
                            runtime.mode_started = None;
                            status.playing = mode == Mode::Playing;
                            // 回执先到时旧心跳仍可能留在观测值中；等待下一条心跳建立本模式基线。
                            status.observed_mode = None;
                            status.applied_revision = status.applied_revision.max(revision);
                            super::code_apply::mode_applied(runtime, mode, revision);
                            runtime.camera = None;
                            runtime.overlay_generation = 0;
                            runtime.tools_generation = 0;
                            feedback(
                                world,
                                if status.playing {
                                    "正在预览当前工程；点击顶部“返回”继续编辑"
                                } else {
                                    "已返回编辑，工程实例与作者草稿继续保留"
                                },
                            );
                        }
                    }
                    Message::Status {
                        world_id,
                        revision,
                        frame,
                        ticks,
                        mode,
                    } if world_id == status.world_id && revision <= runtime.sent_revision => {
                        status.frames = frame;
                        status.ticks = ticks;
                        status.observed_mode = Some(mode);
                        // 心跳只观测帧与玩法ticks；数据和模式必须各自收到可靠事务回执。
                    }
                    Message::Failure {
                        request_id,
                        code,
                        message,
                    } => {
                        let error = format!("{code}：{message}");
                        if runtime.waiting_mode.is_some_and(|(id, _)| id == request_id) {
                            // 关闭所需的返回编辑若被明确拒绝，不能每帧重发并把用户困在隐藏的确认层。
                            // 由精确Job正常收口恢复作者，再走原保存/放弃/取消事务。
                            if runtime.close_return_requested
                                && runtime.waiting_mode == Some((request_id, Mode::Editing))
                                && let Some(active) = &runtime.active
                            {
                                active.control.cancel();
                            }
                            runtime.waiting_mode = None;
                            runtime.mode_started = None;
                        }
                        if runtime.snapshot_in_flight == Some(request_id) {
                            runtime.snapshot_in_flight = None;
                            runtime.snapshot_started = None;
                        }
                        runtime.pending_play = false;
                        status.error = Some(error.clone());
                        feedback(world, &error);
                    }
                    Message::RequestMode { mode } => {
                        if mode == Mode::Editing {
                            runtime.pending_play = false;
                            runtime.pending_edit = true;
                        } else if !status.playing && runtime.hotpatch.flight.is_none() {
                            if let Some(reason) = play_blocker(world) {
                                feedback(world, &reason);
                            } else {
                                runtime.pending_play = true;
                            }
                        }
                    }
                    Message::Stopped => (),
                    _ => (),
                }
            }
        }
    }
    if runtime
        .active
        .as_ref()
        .is_none_or(|active| !active.thread.is_finished())
    {
        return;
    }
    let active = runtime.active.take().expect("工作槽已检查");
    let compilation_stopped = runtime.compilation_stopped && active.control.is_cancelled();
    let result = active
        .result
        .try_recv()
        .unwrap_or_else(|_| Err("常驻工作线程未返回退出回执".into()));
    let joined = active.thread.join().is_ok();
    let log = active.control.log_sink();
    if active.control.cleanup_failed() || !joined {
        runtime.cleanup_failure = Some("常驻进程或管道线程退出未确认，禁止启动另一份工程实例".into());
    }
    status.active = false;
    status.ready = false;
    status.playing = false;
    status.completed = status.completed.saturating_add(1);
    runtime.waiting_mode = None;
    runtime.pending_play = false;
    runtime.play_wait = None;
    runtime.pending_edit = false;
    runtime.close_return_started = None;
    runtime.snapshot_in_flight = None;
    runtime.mode_started = None;
    runtime.snapshot_started = None;
    super::hotpatch::reset(runtime, "原运行实例已完整退出，未完成的函数补丁不再具备采用权");
    if let Some(mut mode) = world.get_resource_mut::<StandaloneEditorMode>() {
        mode.set_external_play_active(false);
    }
    if compilation_stopped {
        if let Some(error) = &runtime.cleanup_failure {
            status.error = Some(error.clone());
            if let Some(log) = &log { log.task_message(&format!("停止未完成：{error}")); }
            feedback(world, error);
        } else {
            // 真实工作线程已结束且清理得到确认，取消错误或迟到的Ok都不再作为构建采用结果。
            status.error = None;
            if let Some(log) = &log { log.task_message("工程3D编译已停止，后台任务已完整退出"); }
            feedback(world, "工程3D编译已停止，作者文档和历史保留；点击“预览”可重试");
        }
    } else if let Err(error) = result {
        if let Some(log) = &log { log.task_message(&format!("任务失败：{error}")); }
        let error = log.as_ref().map_or(error.clone(), |log| format!("{}：{error}", log.task_label()));
        // 原工具提示可能继续更新底栏；失败原因同时保留在宿主日志，便于定位实际子进程退出。
        eprintln!("工程视口失败：{error}");
        status.error = Some(error.clone());
        feedback(
            world,
            &format!("工程视口未完成：{error}；作者现场保留，点击“预览”可重试"),
        );
    } else if let Some(error) = status.error.as_deref() {
        if let Some(log) = &log { log.task_message(&format!("任务停止：{error}")); }
        // 超时由宿主主动结束Job，退出码正常也不能覆盖最初的业务故障。
        eprintln!("工程视口因故障停止：{error}");
        feedback(world, error);
    } else {
        if let Some(log) = &log { log.task_message("任务已完整结束"); }
        feedback(world, "常驻工程实例已完整退出，作者文档和历史保留");
    }
}

pub(super) fn send(runtime: &Runtime, mut message: Outgoing, status: &mut EmbeddedStatus) -> bool {
    let Some(active) = &runtime.active else {
        return false;
    };
    if message.generation == 0 {
        message.generation = runtime
            .identity
            .as_ref()
            .map_or(1, |identity| identity.generation);
    }
    match active.sender.try_send(message) {
        Ok(()) => true,
        Err(mpsc::TrySendError::Full(_)) => false,
        Err(mpsc::TrySendError::Disconnected(_)) => {
            status.error = Some("常驻通信工作槽已关闭".into());
            false
        }
    }
}

pub(super) fn request_mode(
    runtime: &mut Runtime,
    status: &mut EmbeddedStatus,
    mode: Mode,
    revision: u64,
) -> bool {
    if runtime.hotpatch.flight.is_some() { return false; }
    if runtime.waiting_mode.is_some() {
        return false;
    }
    let request = runtime.next_request.saturating_add(1);
    if send(
        runtime,
        Outgoing::control(Message::SetMode {
            request_id: request,
            mode,
            required_revision: revision,
        }),
        status,
    ) {
        runtime.next_request = request;
        runtime.waiting_mode = Some((request, mode));
        runtime.waiting_revision = revision;
        runtime.mode_started = Some(Instant::now());
        if mode == Mode::Playing {
            allow_native_preview_activation(status.pid);
        }
        true
    } else {
        false
    }
}

/// 只在用户前台工作台明确进入预览时授权本会话游戏激活一次；不持续抢焦点。
fn allow_native_preview_activation(pid: u32) {
    #[cfg(windows)]
    if pid != 0 {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            AllowSetForegroundWindow, GA_ROOT, GetAncestor, GetForegroundWindow,
            GetWindowThreadProcessId, IsWindow,
        };
        let foreground = unsafe { GetForegroundWindow() };
        if foreground.is_null() { return; }
        let mut foreground_pid = 0;
        unsafe { GetWindowThreadProcessId(foreground, &mut foreground_pid); }
        let own_foreground = foreground_pid == std::process::id();
        let host_foreground = crate::workspace_window::EmbedTarget::from_env()
            .ok().flatten().is_some_and(|target| {
                let parent = target.parent as windows_sys::Win32::Foundation::HWND;
                (unsafe { IsWindow(parent) != 0 && GetAncestor(parent, GA_ROOT) == foreground })
                    && foreground_pid == target.pid
            });
        if own_foreground || host_foreground {
            unsafe { AllowSetForegroundWindow(pid); }
        }
    }
    #[cfg(not(windows))]
    let _ = pid;
}

fn sync_view(
    world: &mut World,
    runtime: &mut Runtime,
    status: &mut EmbeddedStatus,
    geometry: ViewportGeometry,
) {
    if geometry.revision != runtime.geometry_revision {
        let [x, y, width, height] = geometry.rect;
        if send(
            runtime,
            Outgoing::control(Message::SetViewport {
                x,
                y,
                width: width.max(1) as u32,
                height: height.max(1) as u32,
                visible: geometry.visible,
                focused: geometry.focused,
                excluded_rects: geometry.excluded_rects,
                excluded_shapes: geometry.excluded_shapes,
            }),
            status,
        ) {
            runtime.geometry_revision = geometry.revision;
        }
    }
    if status.playing {
        return;
    }
    let display =
        feathrise_editor_map::standalone::viewport_projection::capture_render(world).map(|view| {
            feathrise_editor_protocol::AuthorViewState {
                shading: view.shading,
                wire: view.wire,
                grid: view.grid,
                terrain_flat: view.terrain_flat,
                terrain_grid: view.terrain_grid,
                terrain_backfaces: view.terrain_backfaces,
                physics_debug: view.physics_debug,
            }
        });
    if let Some(view) = display
        && runtime.author_view != Some(view)
        && send(
            runtime,
            Outgoing::control(Message::SetAuthorView { view }),
            status,
        )
    {
        runtime.author_view = Some(view);
    }
    let Some((transform, projection)) = embedded_snapshot::capture_camera(world) else {
        return;
    };
    let (vertical_fov_radians, orthographic_height) = match projection {
        Projection::Perspective(projection) => (projection.fov, None),
        Projection::Orthographic(projection) => {
            (std::f32::consts::FRAC_PI_4, Some(projection.area.height()))
        }
        Projection::Custom(_) => return,
    };
    let camera = CameraState {
        position: transform.translation.to_array(),
        rotation: transform.rotation.to_array(),
        vertical_fov_radians,
        orthographic_height,
    };
    if runtime.camera != Some(camera)
        && send(
            runtime,
            Outgoing::control(Message::SetCamera { camera }),
            status,
        )
    {
        runtime.camera = Some(camera);
    }
}

fn sync_snapshot(world: &mut World, runtime: &mut Runtime, status: &mut EmbeddedStatus) {
    if status.playing
        || runtime.hotpatch.flight.is_some()
        || runtime.waiting_mode.is_some()
        || runtime.snapshot_in_flight.is_some()
        || !runtime.previews.empty()
        || !embedded_snapshot::ready_is_current(world)
    {
        return;
    }
    let Some(snapshot) = world.resource::<EmbeddedSnapshotState>().ready.as_ref() else {
        return;
    };
    if snapshot.author_revision <= runtime.sent_revision {
        return;
    }
    let revision = snapshot.author_revision;
    let pose_targets = snapshot.pose_targets.clone();
    let pose_map = feathrise_map_content::MapId(snapshot.map_id.clone());
    let changed_map = runtime
        .map_id
        .as_ref()
        .is_some_and(|id| id != &snapshot.map_id);
    let generation = runtime
        .identity
        .as_ref()
        .map_or(1, |identity| identity.generation)
        .saturating_add(u64::from(changed_map));
    if send(
        runtime,
        Outgoing {
            generation,
            message: Message::Snapshot {
                transaction: revision,
                revision,
                map_id: snapshot.map_id.clone(),
                format: feathrise_map_content::embedded_snapshot::SCENE_SNAPSHOT_FORMAT.into(),
            },
            payload: Some(snapshot.bytes.clone()),
        },
        status,
    ) {
        // 只有完整快照进入唯一队列才推进代次；旧地图回执无法越过作者切图身份门。
        runtime
            .identity
            .as_mut()
            .expect("活动会话身份存在")
            .generation = generation;
        runtime.map_id = Some(snapshot.map_id.clone());
        runtime.sent_revision = revision;
        runtime.snapshot_in_flight = Some(revision);
        runtime.snapshot_started = Some(Instant::now());
        feathrise_editor_map::standalone::embedded_author_preview::snapshot_sent(
            world, runtime.identity.as_ref().unwrap().session, generation, revision,
            pose_map, pose_targets,
        );
        super::preview_sources::invalidate(world);
    }
}

pub(super) fn feedback(world: &mut World, message: &str) {
    world.write_message(SavedPlayFeedback(message.into()));
    world.resource_mut::<ViewportModeState>().status = message.into();
}

fn sync_overlay(world: &mut World, runtime: &mut Runtime, status: &mut EmbeddedStatus) {
    if status.playing || runtime.waiting_mode.is_some() {
        return;
    }
    let overlay =
        world.resource::<feathrise_editor_map::standalone::embedded_overlay::EmbeddedOverlay>();
    if overlay.generation == runtime.overlay_generation
        || overlay.revision != status.applied_revision
    {
        return;
    }
    let generation = overlay.generation;
    let diagnostic = overlay.error.clone();
    let lines = overlay
        .lines
        .iter()
        .map(|line| feathrise_editor_protocol::OverlayLine {
            start: line.start,
            end: line.end,
            start_color: line.start_color,
            end_color: line.end_color,
        })
        .collect();
    if send(
        runtime,
        Outgoing::control(Message::SetOverlay {
            revision: overlay.revision,
            lines,
        }),
        status,
    ) {
        runtime.overlay_generation = generation;
        if let Some(error) = diagnostic {
            feedback(world, &error);
        }
    }
}

pub(super) fn play_blocker(world: &World) -> Option<String> {
    if world
        .get_resource::<crate::native_project_close::NativeProjectCloseState>()
        .is_some_and(|state| state.active())
    {
        return Some("请先完成或取消工程关闭，再进入预览".into());
    }
    if world
        .get_resource::<crate::native_project_menu::NativeProjectMenuState>()
        .is_some_and(|state| state.saving())
    {
        return Some("请先等待工程启动设置保存完成".into());
    }
    feathrise_editor_map::standalone::embedded_play_blocker(world)
        .or_else(|| feathrise_editor_map::standalone::saved_play_blocker(world))
}

#[cfg(test)]
#[path = "close_timeout_tests.rs"]
mod close_timeout_tests;
#[cfg(test)]
#[path = "code_apply_tests.rs"]
mod code_apply_tests;

#[cfg(test)]
#[path = "compilation_tests.rs"]
mod compilation_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use feathrise_editor_protocol::Envelope;

    #[test]
    fn rejected_return_for_close_stops_owned_session_before_showing_author_prompt() {
        let (sender, _commands) = mpsc::sync_channel(1);
        let (events, receiver) = mpsc::sync_channel(1);
        let (_, result) = mpsc::sync_channel(1);
        let (release, wait) = mpsc::sync_channel(1);
        let mut runtime = Runtime::default();
        runtime.identity = Some(identity(1));
        runtime.close_return_requested = true;
        runtime.waiting_mode = Some((7, Mode::Editing));
        runtime.active = Some(Active {
            control: default(),
            sender,
            receiver,
            result,
            thread: thread::spawn(move || {
                let _ = wait.recv();
            }),
        });
        let mut world = World::new();
        world.init_resource::<ViewportModeState>();
        world.init_resource::<Messages<SavedPlayFeedback>>();
        let mut status = EmbeddedStatus {
            active: true,
            playing: true,
            ..default()
        };
        events
            .send(Event::Protocol(Envelope::new(
                identity(1),
                1,
                Message::Failure {
                    request_id: 7,
                    code: "editing-rejected".into(),
                    message: "夹具返回失败".into(),
                },
            )))
            .unwrap();
        poll(&mut world, &mut runtime, &mut status);
        assert!(runtime.active.as_ref().unwrap().control.is_cancelled());
        assert!(status.playing, "未确认进程退出前不能伪造模式回执");
        assert!(runtime.waiting_mode.is_none());
        release.send(()).unwrap();
        runtime.active.take().unwrap().thread.join().unwrap();
    }

    #[test]
    fn acknowledged_job_shutdown_keeps_the_preview_timeout_visible() {
        let (sender, _commands) = mpsc::sync_channel(1);
        let (_events, receiver) = mpsc::sync_channel(1);
        let (finished, result) = mpsc::sync_channel(1);
        finished.send(Ok(())).unwrap();
        let control = crate::build_process::BuildControl::default();
        control.cancel(); // 业务超时已要求正常收口，进程退出0不表示预览采用成功。
        let mut runtime = Runtime::default();
        runtime.active = Some(Active {
            control, sender, receiver, result, thread: thread::spawn(|| {}),
        });
        let mut world = World::new();
        world.init_resource::<ViewportModeState>();
        world.init_resource::<Messages<SavedPlayFeedback>>();
        let mut status = EmbeddedStatus { active: true,
            error: Some("实时预览确认超时".into()), ..default() };
        let deadline = Instant::now() + Duration::from_secs(2);
        while runtime.active.is_some() && Instant::now() < deadline {
            poll(&mut world, &mut runtime, &mut status);
            thread::yield_now();
        }
        assert!(runtime.active.is_none(), "实际工作线程必须已经收口");
        assert!(!status.active);
        assert_eq!(world.resource::<ViewportModeState>().status, "实时预览确认超时");
        assert_eq!(status.error.as_deref(), Some("实时预览确认超时"));
    }

    #[test]
    fn play_wait_budget_starts_after_ready_and_restarts_only_for_real_progress() {
        let now = Instant::now();
        let mut wait = None;
        let mut progress = PlayWaitProgress {
            phase: PlayWaitPhase::Connecting,
            reason: None,
            author_revision: 0,
            ready_revision: 0,
            capture_pending: false,
            sent_revision: 0,
            applied_revision: 0,
        };
        assert_eq!(
            observe_play_wait(&mut wait, progress.clone(), false, now),
            WaitObservation::Changed
        );
        assert_eq!(
            observe_play_wait(
                &mut wait,
                progress.clone(),
                false,
                now + Duration::from_secs(900)
            ),
            WaitObservation::Unchanged
        );
        assert!(
            wait.as_ref().unwrap().last_progress.is_none(),
            "首次构建不能占用草稿预算"
        );

        progress.phase = PlayWaitPhase::Preparing;
        progress.reason = Some("等待植被作者层完整编译");
        progress.author_revision = 4;
        progress.capture_pending = true;
        let connected = now + Duration::from_secs(901);
        assert_eq!(
            observe_play_wait(&mut wait, progress.clone(), true, connected),
            WaitObservation::Changed
        );
        assert_eq!(
            observe_play_wait(
                &mut wait,
                progress.clone(),
                true,
                connected + Duration::from_secs(119)
            ),
            WaitObservation::Unchanged
        );
        assert_eq!(
            wait.as_ref().unwrap().last_progress,
            Some(connected),
            "同阶段和心跳不能续期"
        );

        progress.author_revision = 5;
        assert_eq!(
            observe_play_wait(
                &mut wait,
                progress.clone(),
                true,
                connected + Duration::from_secs(120)
            ),
            WaitObservation::Changed
        );
        progress.reason = Some("正在后台编码作者快照");
        assert_eq!(
            observe_play_wait(
                &mut wait,
                progress.clone(),
                true,
                connected + Duration::from_secs(239)
            ),
            WaitObservation::Changed
        );
        progress.phase = PlayWaitPhase::Applying;
        progress.ready_revision = 5;
        progress.sent_revision = 5;
        progress.capture_pending = false;
        assert_eq!(
            observe_play_wait(
                &mut wait,
                progress.clone(),
                true,
                connected + Duration::from_secs(358)
            ),
            WaitObservation::Changed
        );
        assert_eq!(
            observe_play_wait(
                &mut wait,
                progress.clone(),
                true,
                connected + Duration::from_secs(478)
            ),
            WaitObservation::TimedOut
        );
        assert!(wait.is_none());
        assert_eq!(
            observe_play_wait(
                &mut wait,
                progress,
                true,
                connected + Duration::from_secs(479)
            ),
            WaitObservation::Changed,
            "取消本次等待后同一草稿允许重新计时"
        );
    }

    #[test]
    fn stalled_play_wait_reports_stage_without_spam_or_cancelling_the_game() {
        let mut world = World::new();
        world.init_resource::<ViewportModeState>();
        world.init_resource::<Messages<SavedPlayFeedback>>();
        world.init_resource::<EmbeddedSnapshotState>();
        {
            let mut capture = world.resource_mut::<EmbeddedSnapshotState>();
            capture.enabled = true;
            capture.pending = true;
            capture.revision = 4;
            capture.map_id = Some("empty-map".into());
        }
        let (sender, _commands) = mpsc::sync_channel(4);
        let (_, receiver) = mpsc::sync_channel(1);
        let (_, result) = mpsc::sync_channel(1);
        let mut runtime = Runtime::default();
        runtime.pending_play = true;
        runtime.active = Some(Active {
            control: default(),
            sender,
            receiver,
            result,
            thread: thread::spawn(|| {}),
        });
        let mut status = EmbeddedStatus::default();
        let now = Instant::now();
        let selected = ViewportMode::EmbeddedWorld;
        present_play_wait(&mut world, &mut runtime, &status, selected, now);
        assert!(
            world
                .resource::<ViewportModeState>()
                .status
                .contains("等待运行实例完成构建和连接")
        );
        world.resource_mut::<Messages<SavedPlayFeedback>>().clear();
        present_play_wait(
            &mut world,
            &mut runtime,
            &status,
            selected,
            now + Duration::from_secs(900),
        );
        assert!(runtime.pending_play);
        assert_eq!(world.resource::<Messages<SavedPlayFeedback>>().len(), 0);

        status.ready = true;
        let ready = now + Duration::from_secs(901);
        present_play_wait(&mut world, &mut runtime, &status, selected, ready);
        assert!(
            world
                .resource::<ViewportModeState>()
                .status
                .contains("准备草稿（修订4）")
        );
        world.resource_mut::<Messages<SavedPlayFeedback>>().clear();
        present_play_wait(
            &mut world,
            &mut runtime,
            &status,
            selected,
            ready + Duration::from_secs(119),
        );
        assert_eq!(world.resource::<Messages<SavedPlayFeedback>>().len(), 0);
        present_play_wait(
            &mut world,
            &mut runtime,
            &status,
            selected,
            ready + Duration::from_secs(120),
        );
        assert!(!runtime.pending_play);
        assert!(runtime.play_wait.is_none());
        assert!(
            world
                .resource::<ViewportModeState>()
                .status
                .contains("点击“预览”可重试")
        );
        assert_eq!(world.resource::<EmbeddedSnapshotState>().revision, 4);
        assert!(!runtime.active.as_ref().unwrap().control.is_cancelled());
        assert!(status.error.is_none());

        runtime.pending_play = true;
        present_play_wait(
            &mut world,
            &mut runtime,
            &status,
            selected,
            ready + Duration::from_secs(121),
        );
        assert!(runtime.play_wait.is_some());
        world.resource_mut::<EmbeddedSnapshotState>().error = Some("捕获失败详情".into());
        present_capture_diagnostic(&mut world, &mut runtime, &status, selected);
        present_play_wait(
            &mut world,
            &mut runtime,
            &status,
            selected,
            ready + Duration::from_secs(122),
        );
        assert!(
            world
                .resource::<ViewportModeState>()
                .status
                .contains("捕获失败详情")
        );
        assert!(runtime.play_wait.is_none());
        status.error = Some("真实进程失败".into());
        feedback(&mut world, "真实进程失败");
        world.resource_mut::<EmbeddedSnapshotState>().error = None;
        present_capture_diagnostic(&mut world, &mut runtime, &status, selected);
        present_play_wait(
            &mut world,
            &mut runtime,
            &status,
            selected,
            ready + Duration::from_secs(123),
        );
        assert_eq!(world.resource::<ViewportModeState>().status, "真实进程失败");
        runtime.active.take().unwrap().thread.join().unwrap();
    }

    #[test]
    fn coordinator_keeps_capture_failure_visible_after_progress_and_ready_then_recovers() {
        let (sender, _commands) = mpsc::sync_channel(4);
        let (events, receiver) = mpsc::sync_channel(4);
        let (_, result) = mpsc::sync_channel(1);
        let (release, wait) = mpsc::sync_channel(1);
        let mut runtime = Runtime::default();
        runtime.identity = Some(identity(1));
        runtime.active = Some(Active {
            control: default(),
            sender,
            receiver,
            result,
            thread: thread::spawn(move || {
                let _ = wait.recv();
            }),
        });
        let mut world = World::new();
        world.init_resource::<ViewportModeState>();
        world.resource_mut::<ViewportModeState>().selected = ViewportMode::EmbeddedWorld;
        world.init_resource::<ViewportGeometry>();
        world.init_resource::<ViewportModeAvailability>();
        world.init_resource::<Messages<RunSavedProject>>();
        world.init_resource::<Messages<SavedPlayFeedback>>();
        world.init_resource::<EmbeddedAuthorInput>();
        world.init_resource::<EmbeddedSnapshotState>();
        world
            .init_resource::<feathrise_editor_map::standalone::embedded_overlay::EmbeddedOverlay>();
        world.init_resource::<feathrise_editor_map::standalone::embedded_tools::EmbeddedAuthorTools>();
        world.insert_resource(EmbeddedStatus {
            active: true,
            ..default()
        });
        world.insert_non_send(runtime);
        world.resource_mut::<EmbeddedSnapshotState>().error = Some("夹具快照超过128MiB".into());
        let mut schedule = Schedule::default();
        schedule.add_systems(drive);

        schedule.run(&mut world);
        // UI直接呈现这一状态；必须以实际协调流验证，不能只测错误字符串生成函数。
        let diagnostic = world.resource::<ViewportModeState>().status.clone();
        assert!(diagnostic.contains("当前草稿未同步"));
        assert!(diagnostic.contains("夹具快照超过128MiB"));
        assert_eq!(world.resource::<Messages<SavedPlayFeedback>>().len(), 1);

        world.resource_mut::<Messages<SavedPlayFeedback>>().clear();
        events
            .send(Event::Progress("工程构建完成，正在连接".into()))
            .unwrap();
        schedule.run(&mut world);
        assert_eq!(world.resource::<ViewportModeState>().status, diagnostic);
        assert_eq!(world.resource::<Messages<SavedPlayFeedback>>().len(), 2);

        world.resource_mut::<Messages<SavedPlayFeedback>>().clear();
        events
            .send(Event::Protocol(Envelope::new(
                identity(1),
                1,
                Message::Ready {
                    pid: 123,
                    hwnd: 456,
                    world_id: 55,
                    build_identity: "fixture".into(),
                    capabilities: Vec::new(),
                },
            )))
            .unwrap();
        schedule.run(&mut world);
        assert!(world.resource::<EmbeddedStatus>().ready);
        assert_eq!(world.resource::<ViewportModeState>().status, diagnostic);
        assert_eq!(world.resource::<Messages<SavedPlayFeedback>>().len(), 2);

        world.resource_mut::<Messages<SavedPlayFeedback>>().clear();
        schedule.run(&mut world);
        schedule.run(&mut world);
        assert_eq!(world.resource::<ViewportModeState>().status, diagnostic);
        assert_eq!(world.resource::<Messages<SavedPlayFeedback>>().len(), 0);

        {
            let mut capture = world.resource_mut::<EmbeddedSnapshotState>();
            capture.error = None;
            capture.pending = true;
        }
        schedule.run(&mut world);
        assert_eq!(
            world.resource::<ViewportModeState>().status,
            "作者草稿已更新，正在重新准备工程视口快照"
        );
        assert_eq!(world.resource::<Messages<SavedPlayFeedback>>().len(), 1);
        world.resource_mut::<Messages<SavedPlayFeedback>>().clear();
        schedule.run(&mut world);
        assert_eq!(world.resource::<Messages<SavedPlayFeedback>>().len(), 0);

        world.resource_mut::<EmbeddedSnapshotState>().error = Some("新的作者来源错误".into());
        events
            .send(Event::Protocol(Envelope::new(
                identity(1),
                2,
                Message::Failure {
                    request_id: 0,
                    code: "guest-control".into(),
                    message: "实际运行实例连接已断开".into(),
                },
            )))
            .unwrap();
        schedule.run(&mut world);
        assert_eq!(
            world.resource::<ViewportModeState>().status,
            "guest-control：实际运行实例连接已断开"
        );
        world.resource_mut::<EmbeddedSnapshotState>().error = None;
        schedule.run(&mut world);
        assert_eq!(
            world.resource::<ViewportModeState>().status,
            "guest-control：实际运行实例连接已断开",
            "作者修复不能掩盖尚未恢复的实际进程故障"
        );

        release.send(()).unwrap();
        world
            .non_send_mut::<Runtime>()
            .active
            .take()
            .unwrap()
            .thread
            .join()
            .unwrap();
    }

    #[test]
    fn snapshot_applied_rearms_unchanged_author_tool_generation() {
        use feathrise_editor_protocol::AuthorToolPreview;
        use feathrise_editor_map::standalone::embedded_tools::EmbeddedAuthorTools;

        let (sender, commands) = mpsc::sync_channel(1);
        let (events, receiver) = mpsc::sync_channel(2);
        let (_, result) = mpsc::sync_channel(1);
        let (release, wait) = mpsc::sync_channel(1);
        let mut runtime = Runtime::default();
        runtime.identity = Some(identity(2));
        runtime.map_id = Some("map-a".into());
        runtime.sent_revision = 12;
        runtime.snapshot_in_flight = Some(12);
        runtime.snapshot_started = Some(Instant::now());
        runtime.tools_generation = 11;
        runtime.active = Some(Active {
            control: default(), sender, receiver, result,
            thread: thread::spawn(move || { let _ = wait.recv(); }),
        });
        let mut world = World::new();
        world.init_resource::<ViewportModeState>();
        world.init_resource::<Messages<SavedPlayFeedback>>();
        world.insert_resource(EmbeddedAuthorTools {
            revision: 99, map_id: Some("map-a".into()), generation: 11,
            preview: AuthorToolPreview::default(), error: None,
        });
        let mut status = EmbeddedStatus {
            ready: true, world_id: 55, applied_revision: 7, ..default()
        };

        // 只有本事务的正确采用回执才能重置发送代次，错误修订不能提前解除完整快照屏障。
        events.send(Event::Protocol(Envelope::new(identity(2), 1,
            Message::SnapshotApplied { transaction: 12, revision: 999 }))).unwrap();
        poll(&mut world, &mut runtime, &mut status);
        assert_eq!(runtime.tools_generation, 11);
        assert_eq!(runtime.snapshot_in_flight, Some(12));
        assert!(runtime.snapshot_started.is_some());
        assert_eq!(status.applied_revision, 7);

        events.send(Event::Protocol(Envelope::new(identity(2), 2,
            Message::SnapshotApplied { transaction: 12, revision: 12 }))).unwrap();
        poll(&mut world, &mut runtime, &mut status);
        assert!(runtime.snapshot_in_flight.is_none());
        assert!(runtime.snapshot_started.is_none());
        assert_eq!(status.applied_revision, 12);
        assert_eq!(runtime.tools_generation, 0);
        assert_eq!(world.resource::<EmbeddedAuthorTools>().generation, 11);

        // 工具集合没有变化也要重发到新正式修订；待采用草稿99不能替换实际采用的12。
        super::super::tools::synchronize(&mut world, &mut runtime, &mut status);
        let sent = commands.try_recv().unwrap();
        assert_eq!(sent.generation, 2);
        assert_eq!(sent.message, Message::SetAuthorTools {
            revision: 12, preview: AuthorToolPreview::default(),
        });
        assert!(sent.payload.is_none());
        assert_eq!(runtime.tools_generation, 11);
        release.send(()).unwrap();
        runtime.active.take().unwrap().thread.join().unwrap();
    }

    #[test]
    fn full_queue_keeps_mode_request_retryable_and_ack_controls_play_state() {
        let (sender, commands) = mpsc::sync_channel(1);
        let (_, receiver) = mpsc::sync_channel(1);
        let (_, result) = mpsc::sync_channel(1);
        let mut runtime = Runtime::default();
        runtime.identity = Some(identity(1));
        runtime.active = Some(Active {
            control: default(),
            sender,
            receiver,
            result,
            thread: thread::spawn(|| {}),
        });
        let mut status = EmbeddedStatus::default();
        assert!(send(
            &runtime,
            Outgoing::control(Message::Stop),
            &mut status
        ));
        assert!(!request_mode(&mut runtime, &mut status, Mode::Playing, 7));
        assert!(runtime.waiting_mode.is_none());
        assert_eq!(runtime.next_request, 0);
        commands.recv().unwrap();
        assert!(request_mode(&mut runtime, &mut status, Mode::Playing, 7));
        assert_eq!(runtime.waiting_mode, Some((1, Mode::Playing)));
        assert!(!status.playing, "发送命令不能提前放开作者输入");
        assert!(!request_mode(&mut runtime, &mut status, Mode::Editing, 0));
        runtime.active.take().unwrap().thread.join().unwrap();
    }

    #[test]
    fn stale_generation_and_wrong_request_cannot_unlock_current_handoff() {
        let (sender, _commands) = mpsc::sync_channel(4);
        let (events, receiver) = mpsc::sync_channel(4);
        let (_, result) = mpsc::sync_channel(1);
        let (release, wait) = mpsc::sync_channel(1);
        let mut runtime = Runtime::default();
        runtime.identity = Some(identity(2));
        runtime.waiting_mode = Some((9, Mode::Playing));
        runtime.sent_revision = 12;
        runtime.waiting_revision = 12;
        runtime.snapshot_in_flight = Some(12);
        runtime.active = Some(Active {
            control: default(),
            sender,
            receiver,
            result,
            thread: thread::spawn(move || {
                let _ = wait.recv();
            }),
        });
        let mut world = World::new();
        world.init_resource::<Messages<SavedPlayFeedback>>();
        world.init_resource::<ViewportModeState>();
        let mut status = EmbeddedStatus {
            ready: true,
            world_id: 55,
            applied_revision: 10,
            ..default()
        };
        events
            .send(Event::Protocol(Envelope::new(
                identity(1),
                1,
                Message::SnapshotApplied {
                    transaction: 12,
                    revision: 999,
                },
            )))
            .unwrap();
        events
            .send(Event::Protocol(Envelope::new(
                identity(2),
                2,
                Message::ModeApplied {
                    request_id: 8,
                    mode: Mode::Playing,
                    revision: 999,
                    world_id: 55,
                },
            )))
            .unwrap();
        events
            .send(Event::Protocol(Envelope::new(
                identity(2),
                3,
                Message::Status {
                    world_id: 55,
                    revision: 12,
                    frame: 30,
                    ticks: 8,
                    mode: Mode::Playing,
                },
            )))
            .unwrap();
        poll(&mut world, &mut runtime, &mut status);
        assert_eq!(status.applied_revision, 10);
        assert_eq!(runtime.snapshot_in_flight, Some(12));
        assert_eq!(runtime.waiting_mode, Some((9, Mode::Playing)));
        assert!(!status.playing);
        assert_eq!(status.observed_mode, Some(Mode::Playing));
        assert_eq!((status.frames, status.ticks), (30, 8));
        events
            .send(Event::Protocol(Envelope::new(
                identity(2),
                4,
                Message::ModeApplied {
                    request_id: 9,
                    mode: Mode::Playing,
                    revision: 12,
                    world_id: 55,
                },
            )))
            .unwrap();
        poll(&mut world, &mut runtime, &mut status);
        assert!(status.playing);
        assert_eq!(
            status.observed_mode, None,
            "模式回执不能沿用之前心跳作为本次基线"
        );
        assert!(runtime.waiting_mode.is_none());
        release.send(()).unwrap();
        runtime.active.take().unwrap().thread.join().unwrap();
    }

    #[test]
    fn changing_build_profile_joins_the_old_job_before_one_new_attempt() {
        changing_build_choice_joins_before_restart(false);
    }

    #[test]
    fn changing_build_backend_joins_the_old_job_before_one_new_attempt() {
        changing_build_choice_joins_before_restart(true);
    }

    fn changing_build_choice_joins_before_restart(change_backend: bool) {
        use feathrise_editor_core::{ProjectBuildProfile, ProjectLaunchSettings};
        let (sender, _commands) = mpsc::sync_channel(4);
        let (_events, receiver) = mpsc::sync_channel(4);
        let (finished, result) = mpsc::sync_channel(1);
        let (release, wait) = mpsc::sync_channel(1);
        let control = crate::build_process::BuildControl::default();
        let thread_control = control.clone();
        let mut runtime = Runtime::default();
        runtime.attempted_selection = 1;
        runtime.attempted_profile = Some(ProjectBuildProfile::Debug);
        runtime.attempted_backend = Some(crate::native_play::ProjectBuildBackend::Dx);
        runtime.active = Some(Active { control, sender, receiver, result,
            thread: thread::spawn(move || { wait.recv().unwrap(); let _ = finished.send(Ok(())); }),
        });
        let mut world = World::new();
        world.init_resource::<ViewportModeState>();
        world.resource_mut::<ViewportModeState>().selected = ViewportMode::EmbeddedWorld;
        world.init_resource::<ViewportGeometry>();
        world.init_resource::<ViewportModeAvailability>();
        world.init_resource::<crate::native_play::NativePlayStatus>();
        world.init_resource::<Messages<RunSavedProject>>();
        world.init_resource::<Messages<SavedPlayFeedback>>();
        world.init_resource::<EmbeddedAuthorInput>();
        world.init_resource::<EmbeddedSnapshotState>();
        world.init_resource::<feathrise_editor_map::standalone::embedded_overlay::EmbeddedOverlay>();
        world.resource_mut::<EmbeddedSnapshotState>().revision = 42;
        let profile = if change_backend { ProjectBuildProfile::Debug } else { ProjectBuildProfile::Release };
        let backend = if change_backend { crate::native_play::ProjectBuildBackend::Native } else { crate::native_play::ProjectBuildBackend::Dx };
        world.insert_resource(backend);
        world.insert_resource(ProjectLaunchSettings { profile, preview: true, switching_profile: true });
        world.insert_resource(EmbeddedStatus { active: true, ready: true, starts: 1, ..default() });
        world.insert_non_send(runtime);
        let mut schedule = Schedule::default();
        schedule.add_systems(drive);
        schedule.run(&mut world);
        assert!(thread_control.is_cancelled());
        assert!(world.non_send::<Runtime>().active.is_some(), "旧线程未确认结束仍须占槽");
        assert_eq!(world.resource::<EmbeddedStatus>().starts, 1);
        release.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while world.non_send::<Runtime>().active.is_some() {
            assert!(Instant::now() < deadline);
            schedule.run(&mut world);
            thread::yield_now();
        }
        // 新任务刻意使用不存在的自有夹具工程：走真实prepare早退，不运行Cargo或生成任何文件。
        let root = std::env::temp_dir().join(format!("profile-absent-{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        assert!(!root.exists());
        world.insert_resource(Settings { target: root.join("target"), project: crate::project::Project {
            id: "profile-fixture".into(), name: "配置收口回归".into(), root: root.clone(), manifest: root.join("Cargo.toml"), package: "absent-fixture".into(),
            context: crate::project::ProjectContext {
                root: root.clone(), id: "profile-fixture".into(), name: "配置收口回归".into(), format: 1,
                asset_roots: vec![root.join("assets")], editor_data: root.join(".editor"), storage_key: "profile-fixture".into(), startup_scene: None,
            },
        }});
        world.resource_mut::<ViewportGeometry>().parent_hwnd = 1;
        schedule.run(&mut world);
        assert_eq!(world.resource::<EmbeddedStatus>().starts, 2);
        assert_eq!(world.non_send::<Runtime>().attempted_profile, Some(profile));
        assert_eq!(world.non_send::<Runtime>().attempted_backend, Some(backend));
        let deadline = Instant::now() + Duration::from_secs(3);
        while world.non_send::<Runtime>().active.is_some() {
            assert!(Instant::now() < deadline);
            schedule.run(&mut world);
            thread::yield_now();
        }
        schedule.run(&mut world);
        assert_eq!(world.resource::<EmbeddedStatus>().starts, 2, "失败不能静默重试或回退Debug");
        assert_eq!(world.resource::<EmbeddedSnapshotState>().revision, 42, "切profile保留原作者来源");
        assert!(!root.exists());
    }

    fn identity(generation: u64) -> SessionIdentity {
        SessionIdentity {
            project_id: "fixture".into(),
            session: 8,
            generation,
        }
    }
}
