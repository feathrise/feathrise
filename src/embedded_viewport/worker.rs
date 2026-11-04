//! 后台会话生命周期：唯一工程构建、实际产物握手、独占Job与有界管道退出确认。

use super::{
    resources::{Event, Outgoing},
    transport::Transport,
};
use crate::{
    build_process::BuildControl, native_process::JobProcess, project::Project,
    viewport_mode::ViewportGeometry,
};
use feathrise_editor_protocol::{
    Disposition, Envelope, Message, SessionGuard, SessionIdentity,
};
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
    thread,
    time::{Duration, Instant},
};

const CAPABILITIES: &[&str] = &[
    "preview-toolbar-v1",
    "author-view-v1",
    "resident-world-v1",
    "scene-snapshot-v1",
    "embedded-viewport-v1",
    "native-preview-window-v1",
    "editor-overlay-v1",
    "author-tools-v1",
    "editor-ui-occlusion-v1",
    "editor-ui-shapes-v1",
    "author-preview-v1",
];

#[derive(Debug, PartialEq, Eq)]
enum StartupReply {
    Ready { world_id: u64, patch_capable: bool },
    Stopped,
}

/// 启动结果只有第一次失败可写；先检查再分类，避免Stopped/断管遮住根因或迟到Ready复活。
/// 直接借用生命周期的原failure槽，生产收包与顺序回归共用此门，不维护第二份错误状态。
fn settle_startup_reply(
    failure: &mut Option<String>,
    receive: impl FnOnce() -> Result<StartupReply, String>,
) -> Option<StartupReply> {
    if failure.is_some() { return None; }
    match receive() {
        Ok(reply) => Some(reply),
        Err(error) => {
            *failure = Some(error);
            None
        }
    }
}

/// 调用前必须通过会话身份与顺序门。启动失败是终止诊断，不能冒充握手或丢掉原错误。
/// 窗口归属只注入查询谓词，生产仍查询本次专属进程的真实 HWND，不借此跳过 OS 核验。
fn classify_startup_reply(
    message: &Envelope,
    expected_build: &str,
    expected_pid: u32,
    stopping: bool,
    owns_window: impl FnOnce(u64, u32) -> bool,
) -> Result<StartupReply, String> {
    match &message.message {
        Message::Ready { pid, hwnd, world_id, capabilities, .. } => {
            SessionGuard::verify_handshake(message, expected_build, CAPABILITIES)
                .map_err(|error| error.to_string())?;
            if *pid != expected_pid || !owns_window(*hwnd, *pid) {
                return Err("运行窗口与本次专属进程身份不一致".into());
            }
            Ok(StartupReply::Ready {
                world_id: *world_id,
                patch_capable: capabilities.iter().any(|capability|
                    capability == feathrise_editor_protocol::FUNCTION_HOTPATCH_CAPABILITY),
            })
        }
        Message::Failure { code, message, .. } => Err(format!("工程启动失败（{code}）：{message}")),
        // 只有宿主已经进入停止流程，Stopped才是合法收口；它永远不建立Ready身份。
        Message::Stopped if stopping => Ok(StartupReply::Stopped),
        _ => Err("运行进程首回执必须为Ready".into()),
    }
}

/// 换代期间只借用上一代的有效心跳证明进程存活，不给旧结果任何采用权限。
/// 游戏要到完整新来源可发布时才切换回执代次，不能用平时的30秒心跳预算抢先否定120秒快照预算。
#[derive(Default)]
struct ReceiveLiveness {
    last_sequence: u64,
    handoff: Option<(SessionIdentity, Instant)>,
}

impl ReceiveLiveness {
    fn begin_handoff(&mut self, previous: SessionIdentity, now: Instant) {
        self.handoff = Some((previous, now));
    }

    fn accept_current(&mut self, sequence: u64) -> bool {
        if sequence <= self.last_sequence {
            return false;
        }
        self.last_sequence = sequence;
        // 新代已开始回执，之后任何旧代数据都不再续命。
        self.handoff = None;
        true
    }

    fn accept_previous_status(
        &mut self,
        message: &Envelope,
        world_id: Option<u64>,
        now: Instant,
    ) -> bool {
        let Some((previous, started)) = self.handoff.as_ref() else {
            return false;
        };
        if now.duration_since(*started) > Duration::from_secs(120) {
            self.handoff = None;
            return false;
        }
        if message.identity != *previous || message.sequence <= self.last_sequence {
            return false;
        }
        // 同一条有序管道中的旧ACK也占顺序号，但不会刷新存活时间或进入UI。
        self.last_sequence = message.sequence;
        if !matches!(message.message, Message::Status { world_id: id, .. } if world_id == Some(id))
        {
            return false;
        }
        true
    }
}

pub(super) fn run(
    project: Project,
    target: PathBuf,
    profile: feathrise_editor_core::ProjectBuildProfile,
    prepared: Option<Arc<crate::native_play::PreparedGame>>,
    geometry: ViewportGeometry,
    mut identity: SessionIdentity,
    control: BuildControl,
    commands: mpsc::Receiver<Outgoing>,
    events: mpsc::SyncSender<Event>,
) -> Result<(), String> {
    let prepared = match prepared {
        Some(prepared) => {
            if !prepared.matches_project(&project, profile) {
                return Err("待应用版本与本次工程或构建配置不一致，拒绝启动".into());
            }
            prepared
        }
        None => {
            let _ = events.try_send(Event::Progress(
                "正在构建工程，首次准备可能较久；作者文档保持可用".into(),
            ));
            Arc::new(crate::native_play::prepare_game(&project, &target, None, profile, &control)?)
        }
    };
    if let Some(baseline) = &prepared.hotpatch {
        // 基线事件必须先于Ready可靠投递；不能因作者帧暂忙而丢失真实构建与运行实例的联系。
        let mut event = Event::HotpatchBaseline(baseline.clone());
        loop {
            control.check()?;
            match events.try_send(event) {
                Ok(()) => break,
                Err(mpsc::TrySendError::Full(returned)) => { event = returned; thread::sleep(Duration::from_millis(5)); }
                Err(mpsc::TrySendError::Disconnected(_)) => return Err("作者会话已结束，未启动旧函数补丁基线".into()),
            }
        }
    }
    let _ = events.try_send(Event::Progress(
        "工程构建完成，正在建立专用通信并核对实际产物身份".into(),
    ));
    let build_identity = hash_file(&prepared.executable, &control)?;
    let mut command = prepared.embedded_command(&project, &control)?;
    command.args([
        "--feathrise-embedded",
        "--session",
        &identity.session.to_string(),
        "--generation",
        &identity.generation.to_string(),
        "--parent-window",
        &geometry.parent_hwnd.to_string(),
        "--parent-pid",
        &geometry.parent_pid.to_string(),
    ]);
    control.check()?;
    let (process, write, read, output, errors) = match JobProcess::spawn_runtime_command(&command) {
        Ok(value) => value,
        Err(error) => {
            if error.cleanup_unconfirmed {
                control.mark_cleanup_failed();
            }
            return Err(error.message);
        }
    };
    let stdout_log = control.log_sink();
    let stderr_log = control.log_sink();
    let stdout = thread::spawn(move || drain_log(crate::project_log::LogReader::new(
        output, stdout_log, crate::project_log::LogSource::Stdout,
    )));
    let stderr = thread::spawn(move || drain_log(crate::project_log::LogReader::new(
        errors, stderr_log, crate::project_log::LogSource::Stderr,
    )));
    let mut transport = match Transport::spawn(read, write) {
        Ok(value) => value,
        Err(error) => {
            let cleanup = finish_failed_start(&process, stdout, stderr, &control);
            return Err(format!(
                "{error}{}",
                cleanup
                    .err()
                    .map_or(String::new(), |error| format!("；{error}"))
            ));
        }
    };
    let mut sequence = 1;
    let mut offered_capabilities: Vec<String> = CAPABILITIES.iter().map(|value| (*value).into()).collect();
    if prepared.hotpatch.is_some() { offered_capabilities.push(feathrise_editor_protocol::FUNCTION_HOTPATCH_CAPABILITY.into()); }
    let mut patch_negotiated = false;
    let mut pending = Some(Envelope::new(
        identity.clone(),
        sequence,
        Message::Hello {
            build_identity: build_identity.clone(),
            capabilities: offered_capabilities,
        },
    ));
    let mut guard = SessionGuard::new(identity.clone());
    let mut liveness = ReceiveLiveness::default();
    let mut outgoing_events = VecDeque::new();
    let started = Instant::now();
    let mut last_received = started;
    let mut stop_started = None;
    let mut failure = None;
    let mut ready = false;
    let mut world_id = None;
    let mut stopped = false;
    let mut exit = None;
    let mut terminated = false;
    loop {
        if ready && last_received.elapsed() > Duration::from_secs(30) && failure.is_none() {
            failure = Some("运行进程30秒没有有效心跳，正在确认收口；作者现场保留".into());
        }
        #[cfg(feature = "validation")]
        if control.crash_requested() && !terminated {
            failure = Some(match process.terminate() {
                Ok(()) => "本轮自有夹具已通过专属Job注入实际运行进程崩溃".into(),
                Err(error) => error,
            });
            terminated = true;
        }
        if control.is_cancelled() && stop_started.is_none() {
            stop_started = Some(Instant::now());
        }
        if !ready && started.elapsed() > Duration::from_secs(120) && failure.is_none() {
            failure = Some("工程未在120秒内完成常驻视口握手；请确认工程SDK支持此协议".into());
        }
        if failure.is_some() && stop_started.is_none() {
            stop_started = Some(Instant::now());
        }

        // 一个尚未写入的完整帧占住唯一待发槽，不能绕过它重排模式与作者修订。
        if let Some(message) = pending.take() {
            match transport.try_send(message) {
                Ok(()) => (),
                Err(async_channel::TrySendError::Full(message)) => pending = Some(message),
                Err(async_channel::TrySendError::Closed(_)) => {
                    failure.get_or_insert("工程控制管道已关闭".into());
                }
            }
        }
        // Hello已占唯一待发槽；握手前不取业务邮箱，明确停止仍可发送Stop并有界收口。
        if pending.is_none() && exit.is_none() && (ready || stop_started.is_some()) {
            if stop_started.is_some() && !stopped {
                sequence += 1;
                pending = Some(Envelope::new(identity.clone(), sequence, Message::Stop));
                stopped = true;
            } else if ready && stop_started.is_none() {
                match commands.try_recv() {
                    Ok(command) => {
                        if command.generation != identity.generation {
                            if command.generation != identity.generation.saturating_add(1)
                                || !matches!(command.message, Message::Snapshot { .. })
                            {
                                failure = Some("仅完整切图快照可以推进一个会话代次".into());
                                continue;
                            }
                            if let Err(error) = guard.advance_generation(command.generation) {
                                failure = Some(error.to_string());
                                continue;
                            }
                            liveness.begin_handoff(identity.clone(), Instant::now());
                            identity.generation = command.generation;
                        }
                        sequence += 1;
                        pending = Some(Envelope {
                            identity: identity.clone(),
                            sequence,
                            message: command.message,
                            payload: command.payload,
                        });
                    }
                    Err(mpsc::TryRecvError::Disconnected) => stop_started = Some(Instant::now()),
                    Err(mpsc::TryRecvError::Empty) => (),
                }
            }
        }

        for _ in 0..8 {
            if outgoing_events.len() >= 16 {
                break;
            }
            let message = match transport.try_recv() {
                Ok(Some(message)) => message,
                Ok(None) => break,
                Err(error) => {
                    if exit.is_none() && stop_started.is_none() {
                        if ready {
                            failure.get_or_insert(error);
                        } else {
                            let _ = settle_startup_reply(&mut failure, || Err(error));
                        }
                    }
                    break;
                }
            };
            match guard.accept(&message) {
                Ok(Disposition::StaleGeneration) => {
                    let now = Instant::now();
                    if liveness.accept_previous_status(&message, world_id, now) {
                        last_received = now;
                    }
                    continue;
                }
                Ok(Disposition::Current) => {
                    if !liveness.accept_current(message.sequence) {
                        failure.get_or_insert("换代回执顺序号倒退".into());
                        break;
                    }
                }
                Err(error) => {
                    failure.get_or_insert(error.to_string());
                    break;
                }
            }
            last_received = Instant::now();
            if !ready {
                // 首个真实失败锁定启动结果；后续Stopped、断管或迟到Ready都不能覆盖/复活它。
                match settle_startup_reply(&mut failure, || classify_startup_reply(
                    &message, &build_identity, process.pid(), stop_started.is_some(), owned_window,
                )) {
                    Some(StartupReply::Ready { world_id: id, patch_capable }) => {
                        if stop_started.is_some() { continue; }
                        world_id = Some(id);
                        patch_negotiated = prepared.hotpatch.is_some() && patch_capable;
                        ready = true;
                    }
                    Some(StartupReply::Stopped) => continue,
                    None => break,
                }
            } else {
                match message.message {
                    Message::Ready { .. } | Message::Hello { .. } => {
                        failure = Some("同一会话重复握手".into());
                        break;
                    }
                    Message::ModeApplied { world_id: id, .. }
                    | Message::Status { world_id: id, .. }
                        if world_id != Some(id) =>
                    {
                        failure = Some("常驻工程World身份意外变化".into());
                        break;
                    }
                    Message::Stopped => {
                        if stop_started.is_none() {
                            stop_started = Some(Instant::now());
                        }
                    }
                    Message::HotPatchReady { pid, world_id: id, aslr_reference, ref base_build }
                        if !patch_negotiated || pid != process.pid() || world_id != Some(id)
                            || aslr_reference == 0 || base_build != &build_identity => {
                        failure = Some("函数热更新就绪回执不属于本次实际运行进程、World或基线".into());
                        break;
                    }
                    Message::CodePatchResult { pid, world_id: id, .. }
                        if !patch_negotiated || pid != process.pid() || world_id != Some(id) => {
                        failure = Some("函数热更新结果不属于本次实际运行进程与World".into());
                        break;
                    }
                    Message::ModeApplied { .. }
                    | Message::Status { .. }
                    | Message::SnapshotApplied { .. }
                    | Message::PreviewApplied { .. }
                    | Message::Failure { .. }
                    | Message::RequestMode { .. } => (),
                    Message::HotPatchReady { .. } | Message::CodePatchResult { .. } => (),
                    _ => {
                        failure = Some("工程执行端发送了宿主专属命令，拒绝越过通信方向边界".into());
                        break;
                    }
                }
            }
            outgoing_events.push_back(Event::Protocol(message));
        }
        while let Some(event) = outgoing_events.pop_front() {
            match events.try_send(event) {
                Ok(()) => (),
                Err(mpsc::TrySendError::Full(event)) => {
                    outgoing_events.push_front(event);
                    break;
                }
                Err(mpsc::TrySendError::Disconnected(_)) => {
                    control.cancel();
                    outgoing_events.clear();
                    break;
                }
            }
        }
        if exit.is_none() {
            match process.try_exit() {
                Ok(code) => exit = code,
                Err(error) => {
                    failure.get_or_insert(error);
                }
            }
        }
        if exit.is_some() && stop_started.is_none() {
            stop_started = Some(Instant::now());
        }
        if stop_started.is_some_and(|time| time.elapsed() > Duration::from_secs(2)) && !terminated {
            if let Err(error) = process.terminate() {
                failure.get_or_insert(error);
            }
            terminated = true;
        }
        let count = match process.active_processes() {
            Ok(count) => count,
            Err(error) => {
                failure.get_or_insert(error);
                u32::MAX
            }
        };
        if exit.is_some() && count == 0 {
            transport.shutdown();
            if transport.is_finished() && stdout.is_finished() && stderr.is_finished() {
                break;
            }
        }
        if stop_started.is_some_and(|time| time.elapsed() > Duration::from_secs(8)) {
            control.mark_cleanup_failed();
            return Err(
                "运行Job、专用管道或日志线程未在退出预算内收口；禁止重启以免遗留会话".into(),
            );
        }
        thread::sleep(Duration::from_millis(5)); // 仅后台协调线程等待；作者帧始终只try_recv。
    }
    let transport_result = transport.finish();
    let output = stdout.join().map_err(|_| "运行进程stdout线程异常")??;
    let errors = stderr.join().map_err(|_| "运行进程stderr线程异常")??;
    if let Some(error) = failure {
        if !control.is_cancelled() {
            return Err(format!("{error}\n{errors}\n{output}"));
        }
    }
    if !control.is_cancelled() {
        transport_result?;
        if exit != Some(0) {
            return Err(format!("运行进程异常退出 {:?}\n{errors}\n{output}", exit));
        }
    }
    Ok(())
}

/// 即使通信线程创建失败，也要确认专属Job与已启动日志线程退出，才允许UI重试。
fn finish_failed_start(
    process: &JobProcess,
    stdout: thread::JoinHandle<Result<String, String>>,
    stderr: thread::JoinHandle<Result<String, String>>,
    control: &BuildControl,
) -> Result<(), String> {
    let termination = process.terminate();
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(8) {
        if matches!(process.try_exit(), Ok(Some(_)))
            && matches!(process.active_processes(), Ok(0))
            && stdout.is_finished()
            && stderr.is_finished()
        {
            let _ = stdout.join();
            let _ = stderr.join();
            return termination;
        }
        thread::sleep(Duration::from_millis(5));
    }
    control.mark_cleanup_failed();
    Err("通信初始化失败后的专属进程收口未确认，禁止重启".into())
}

fn owned_window(hwnd: u64, pid: u32) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowThreadProcessId, IsWindow};
    let hwnd = hwnd as usize as windows_sys::Win32::Foundation::HWND;
    let mut actual = 0;
    // SAFETY：只查询句柄归属，不对跨进程窗口发同步消息。
    unsafe {
        IsWindow(hwnd) != 0 && GetWindowThreadProcessId(hwnd, &mut actual) != 0 && actual == pid
    }
}

fn hash_file(path: &Path, control: &BuildControl) -> Result<String, String> {
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        control.check()?;
        let count = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn drain_log(mut file: impl Read) -> Result<String, String> {
    let mut tail = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        let count = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        tail.extend_from_slice(&buffer[..count]);
        if tail.len() > 16 * 1024 {
            tail.drain(..tail.len() - 16 * 1024);
        }
    }
    Ok(String::from_utf8_lossy(&tail).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use feathrise_editor_protocol::Mode;

    fn identity(generation: u64) -> SessionIdentity {
        SessionIdentity {
            project_id: "handoff-liveness".into(),
            session: 7,
            generation,
        }
    }

    fn status(sequence: u64) -> Envelope {
        Envelope::new(
            identity(2),
            sequence,
            Message::Status {
                world_id: 42,
                revision: 5,
                frame: sequence,
                ticks: 0,
                mode: Mode::Editing,
            },
        )
    }

    fn startup_ready() -> Envelope {
        Envelope::new(identity(1), 1, Message::Ready {
            pid: 44, hwnd: 88, world_id: 9, build_identity: "artifact-sha256".into(),
            capabilities: CAPABILITIES.iter().map(|value| (*value).into()).collect(),
        })
    }

    #[test]
    fn old_sdk_missing_native_capability_remains_the_startup_failure_after_shutdown_and_late_ready() {
        let ready = startup_ready();
        let mut healthy_failure = None;
        assert_eq!(settle_startup_reply(&mut healthy_failure, || classify_startup_reply(
            &ready, "artifact-sha256", 44, false, |hwnd, pid| (hwnd, pid) == (88, 44),
        )), Some(StartupReply::Ready { world_id: 9, patch_capable: false }));
        assert!(healthy_failure.is_none());

        let mut old_ready = startup_ready();
        let Message::Ready { capabilities, .. } = &mut old_ready.message else { unreachable!() };
        capabilities.retain(|capability| capability != "native-preview-window-v1");
        assert_eq!(capabilities.len() + 1, CAPABILITIES.len());
        let mut failure = None;
        let mut guard = SessionGuard::new(identity(1));
        assert_eq!(guard.accept(&old_ready).unwrap(), Disposition::Current);
        assert_eq!(settle_startup_reply(&mut failure, || classify_startup_reply(
            &old_ready, "artifact-sha256", 44, false,
            |_, _| panic!("缺少原生预览能力时不能采用窗口"),
        )), None);
        let original = "IPC 身份不符：运行实例缺少必需能力：native-preview-window-v1";
        assert_eq!(failure.as_deref(), Some(original));

        // 这是旧SDK的实际失败顺序；每一步复用生产首错门，不能只测试单条分类结果。
        let stopped = Envelope::new(identity(1), 2, Message::Stopped);
        assert_eq!(guard.accept(&stopped).unwrap(), Disposition::Current);
        assert_eq!(settle_startup_reply(&mut failure, || classify_startup_reply(
            &stopped, "artifact-sha256", 44, true,
            |_, _| panic!("停止回执不能建立窗口身份"),
        )), None);
        assert_eq!(failure.as_deref(), Some(original));
        assert_eq!(settle_startup_reply(&mut failure, || Err("工程 IPC 接收通道已关闭".into())), None);
        assert_eq!(failure.as_deref(), Some(original));

        let mut late_ready = startup_ready();
        late_ready.sequence = 3;
        assert_eq!(guard.accept(&late_ready).unwrap(), Disposition::Current);
        assert_eq!(settle_startup_reply(&mut failure, || classify_startup_reply(
            &late_ready, "artifact-sha256", 44, true,
            |_, _| panic!("首错已终止启动，迟到Ready不得再次查询或采用窗口"),
        )), None);
        assert_eq!(failure.as_deref(), Some(original));
    }

    #[test]
    fn startup_failure_preserves_the_original_diagnostic_without_claiming_ready() {
        let failure = Envelope::new(identity(1), 1, Message::Failure {
            request_id: 0, code: "native-window".into(),
            message: "工程与作者窗口DPI模式不同\n原始诊断完整保留".into(),
        });
        let mut guard = SessionGuard::new(identity(1));
        assert_eq!(guard.accept(&failure).unwrap(), Disposition::Current);
        assert_eq!(classify_startup_reply(&failure, "artifact-sha256", 44, false,
            |_, _| panic!("Failure不建立窗口或Ready身份")),
            Err("工程启动失败（native-window）：工程与作者窗口DPI模式不同\n原始诊断完整保留".into()));
    }

    #[test]
    fn startup_ready_requires_the_actual_build_every_capability_and_owned_process_window() {
        let ready = startup_ready();
        let owns = |hwnd, pid| (hwnd, pid) == (88, 44);
        assert_eq!(classify_startup_reply(&ready, "artifact-sha256", 44, false, owns).unwrap(),
            StartupReply::Ready { world_id: 9, patch_capable: false });
        for wrong_build in ["old-artifact", ""] {
            assert!(classify_startup_reply(&ready, wrong_build, 44, false,
                |_, _| panic!("构建身份未核对通过前不能查询窗口")).is_err());
        }
        for &missing in CAPABILITIES {
            let mut incomplete = startup_ready();
            let Message::Ready { capabilities, .. } = &mut incomplete.message else { unreachable!() };
            capabilities.retain(|capability| capability.as_str() != missing);
            assert!(classify_startup_reply(&incomplete, "artifact-sha256", 44, false,
                |_, _| panic!("缺少能力时不能采用窗口")).unwrap_err().contains(missing));
        }
        let mut foreign_pid = startup_ready();
        let Message::Ready { pid, .. } = &mut foreign_pid.message else { unreachable!() };
        *pid = 45;
        assert!(classify_startup_reply(&foreign_pid, "artifact-sha256", 44, false,
            |_, _| panic!("不能查询另一进程并把它当作本次Job")).is_err());
        let mut foreign_window = startup_ready();
        let Message::Ready { hwnd, .. } = &mut foreign_window.message else { unreachable!() };
        *hwnd = 89;
        assert!(classify_startup_reply(&foreign_window, "artifact-sha256", 44, false, owns).is_err());
        assert!(classify_startup_reply(&ready, "artifact-sha256", 44, false, |_, _| false).is_err());

        let mut patch_ready = startup_ready();
        let Message::Ready { capabilities, .. } = &mut patch_ready.message else { unreachable!() };
        capabilities.push(feathrise_editor_protocol::FUNCTION_HOTPATCH_CAPABILITY.into());
        assert_eq!(classify_startup_reply(&patch_ready, "artifact-sha256", 44, false, owns).unwrap(),
            StartupReply::Ready { world_id: 9, patch_capable: true });
    }

    #[test]
    fn startup_status_hello_and_ack_never_substitute_for_ready_even_while_stopping() {
        for message in [
            status(1).message,
            Message::Hello { build_identity: "artifact-sha256".into(),
                capabilities: CAPABILITIES.iter().map(|value| (*value).into()).collect() },
            Message::SnapshotApplied { transaction: 7, revision: 7 },
        ] {
            let frame = Envelope::new(identity(1), 1, message);
            for stopping in [false, true] {
                assert_eq!(classify_startup_reply(&frame, "artifact-sha256", 44, stopping,
                    |_, _| panic!("非Ready消息不能进入窗口采用")),
                    Err("运行进程首回执必须为Ready".into()));
            }
        }
    }

    #[test]
    fn startup_stopped_is_only_a_shutdown_ack_when_the_host_has_already_started_stopping() {
        let stopped = Envelope::new(identity(1), 1, Message::Stopped);
        assert!(classify_startup_reply(&stopped, "artifact-sha256", 44, false,
            |_, _| panic!("Stopped不能建立窗口身份")).is_err());
        assert_eq!(classify_startup_reply(&stopped, "artifact-sha256", 44, true,
            |_, _| panic!("取消回执不能建立Ready身份")).unwrap(), StartupReply::Stopped);
    }

    #[test]
    fn handoff_uses_only_ordered_previous_world_status_for_liveness() {
        let now = Instant::now();
        let mut liveness = ReceiveLiveness::default();
        assert!(liveness.accept_current(10));
        assert!(!liveness.accept_previous_status(&status(11), Some(42), now));
        liveness.begin_handoff(identity(2), now);
        assert!(!liveness.accept_previous_status(&status(10), Some(42), now));

        for wrong_identity in [
            SessionIdentity {
                session: 8,
                ..identity(2)
            },
            SessionIdentity {
                project_id: "other".into(),
                ..identity(2)
            },
            identity(1),
        ] {
            let mut message = status(11);
            message.identity = wrong_identity;
            assert!(!liveness.accept_previous_status(&message, Some(42), now));
        }
        assert!(liveness.accept_previous_status(&status(11), Some(42), now));
        assert!(!liveness.accept_previous_status(&status(11), Some(42), now));
        assert!(!liveness.accept_previous_status(&status(12), Some(43), now));
        assert!(!liveness.accept_previous_status(&status(13), None, now));
        assert!(liveness.accept_previous_status(&status(14), Some(42), now));

        for (sequence, message) in [
            (
                15,
                Message::SnapshotApplied {
                    transaction: 5,
                    revision: 5,
                },
            ),
            (
                16,
                Message::ModeApplied {
                    request_id: 1,
                    mode: Mode::Editing,
                    revision: 5,
                    world_id: 42,
                },
            ),
        ] {
            let ack = Envelope::new(identity(2), sequence, message);
            assert!(!liveness.accept_previous_status(&ack, Some(42), now));
            assert!(!liveness.accept_previous_status(&status(sequence), Some(42), now));
        }
        assert!(liveness.accept_previous_status(&status(17), Some(42), now));
    }

    #[test]
    fn first_current_reply_closes_old_generation_liveness_and_preserves_order() {
        let now = Instant::now();
        let mut liveness = ReceiveLiveness::default();
        assert!(liveness.accept_current(10));
        liveness.begin_handoff(identity(2), now);
        assert!(liveness.accept_previous_status(&status(11), Some(42), now));
        assert!(!liveness.accept_current(11));
        assert!(liveness.accept_current(12));
        assert!(!liveness.accept_previous_status(&status(13), Some(42), now));
        assert!(liveness.accept_current(13));
    }

    #[test]
    fn previous_status_cannot_extend_the_snapshot_handoff_budget() {
        let now = Instant::now();
        let mut liveness = ReceiveLiveness::default();
        assert!(liveness.accept_current(10));
        liveness.begin_handoff(identity(2), now);
        // 原30秒预算之外仍可确认进程存活，但旧代不能无限维持一个未采用事务。
        assert!(liveness.accept_previous_status(
            &status(11),
            Some(42),
            now + Duration::from_secs(45)
        ));
        assert!(liveness.accept_previous_status(
            &status(12),
            Some(42),
            now + Duration::from_secs(120)
        ));
        assert!(!liveness.accept_previous_status(
            &status(13),
            Some(42),
            now + Duration::from_secs(121)
        ));
        assert!(liveness.handoff.is_none());
        assert!(!liveness.accept_previous_status(
            &status(14),
            Some(42),
            now + Duration::from_secs(122)
        ));
    }
}
