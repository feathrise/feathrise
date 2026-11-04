//! 真实工程预览的后台所有者：复用普通构建、核对当前EXE SHA、独占Job并确认全部线程退出。
use super::{Event, Request, transport::Transport};
use crate::{build_process::BuildControl, native_process::JobProcess, project::Project};
use feathrise_runtime::project::author_preview::wire::Packet;
use feathrise_editor_core::ProjectBuildProfile;
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

/// 当前已受理请求的协议状态；领域错误对这一修订终态，下一次preview才恢复帧采用权。
struct Expected {
    revision: u64,
    kind: String,
    last: Instant,
    captured: bool,
    terminal: bool,
}
impl Expected {
    fn new(revision: u64, kind: String) -> Self {
        Self {
            revision,
            kind,
            last: Instant::now(),
            captured: false,
            terminal: false,
        }
    }
    fn accepts_frame(&self, revision: u64, kind: &str) -> bool {
        !self.terminal && self.revision == revision && self.kind == kind
    }
    fn fail(&mut self, revision: u64, kind: &str) -> bool {
        if !self.accepts_frame(revision, kind) {
            return false;
        }
        self.terminal = true;
        true
    }
}

pub(super) fn run(
    project: Project,
    target: PathBuf,
    profile: ProjectBuildProfile,
    session: u64,
    control: BuildControl,
    commands: async_channel::Receiver<Request>,
    events: async_channel::Sender<Event>,
) -> Result<(), String> {
    let prepared = crate::native_play::prepare_game(&project, &target, None, profile, &control)?;
    let executable_sha256 = hash_file(&prepared.executable, &control)?;
    let mut command = prepared.embedded_command(&project, &control)?;
    command.args([
        "--feathrise-author-preview",
        "--preview-session",
        &session.to_string(),
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
    let stdout = thread::spawn(move || {
        drain_log(crate::project_log::LogReader::new(
            output,
            stdout_log,
            crate::project_log::LogSource::Stdout,
        ))
    });
    let stderr = thread::spawn(move || {
        drain_log(crate::project_log::LogReader::new(
            errors,
            stderr_log,
            crate::project_log::LogSource::Stderr,
        ))
    });
    let mut transport = match Transport::spawn(read, write) {
        Ok(transport) => transport,
        Err(error) => {
            let cleanup = finish_failed_start(&process, stdout, stderr, &control);
            return Err(format!(
                "{error}{}",
                cleanup.err().map_or(String::new(), |e| format!("；{e}"))
            ));
        }
    };
    let packet = |message: &str, revision: u64, kind: String, data: serde_json::Value| Packet {
        project: project.id.clone(),
        session,
        revision,
        message: message.into(),
        kind,
        data,
    };
    let mut pending = Some(packet(
        "hello",
        0,
        String::new(),
        serde_json::json!({"executable_sha256":executable_sha256}),
    ));
    let started = Instant::now();
    let mut ready = false;
    let mut expected: Option<Expected> = None;
    let mut stop_started: Option<Instant> = None;
    let mut stop_sent = false;
    let mut stopped = false;
    let mut exit = None;
    let mut terminated = false;
    let mut failure: Option<String> = None;
    let mut error_pending = None;
    loop {
        if control.is_cancelled() || events.is_closed() {
            stop_started.get_or_insert_with(Instant::now);
        }
        if !ready && started.elapsed() > Duration::from_secs(120) {
            failure.get_or_insert("工程未在120秒内完成真实预览握手".into());
        }
        if let Some(expected) = &expected
            && !expected.terminal
        {
            let limit = if expected.captured { 30 } else { 120 };
            if expected.last.elapsed() > Duration::from_secs(limit) {
                failure.get_or_insert(format!("工程真实预览{limit}秒未回传当前画面，正在确认收口"));
            }
        }
        if failure.is_some() {
            stop_started.get_or_insert_with(Instant::now);
        }
        if pending.is_none() {
            if stop_started.is_some() && !stop_sent {
                pending = Some(packet(
                    "stop",
                    expected.as_ref().map_or(0, |request| request.revision),
                    String::new(),
                    serde_json::Value::Null,
                ));
            } else if ready && stop_started.is_none() {
                match commands.try_recv() {
                    Ok(request) => {
                        pending = Some(packet(
                            request.message,
                            request.revision,
                            request.kind,
                            request.data,
                        ))
                    }
                    Err(async_channel::TryRecvError::Closed) => {
                        stop_started.get_or_insert_with(Instant::now);
                    }
                    Err(async_channel::TryRecvError::Empty) => (),
                }
            }
        }
        if let Some(outgoing) = pending.take() {
            let message = outgoing.message.clone();
            let revision = outgoing.revision;
            let kind = outgoing.kind.clone();
            match transport.send(outgoing) {
                Ok(()) => match message.as_str() {
                    "preview" => {
                        expected = Some(Expected::new(revision, kind));
                        error_pending = None;
                    }
                    "clear" => {
                        expected = None;
                        error_pending = None;
                    }
                    "stop" => {
                        stop_sent = true;
                        transport.close_sender();
                    }
                    _ => (),
                },
                Err(async_channel::TrySendError::Full(packet)) => pending = Some(packet),
                Err(async_channel::TrySendError::Closed(_)) => {
                    if stop_started.is_none() {
                        failure.get_or_insert("预览专用写管道已关闭".into());
                    }
                }
            }
        }
        // 一次最多接收两份固定尺寸回传，UI堵塞只丢展示帧，控制包与关闭确认持续读取。
        for _ in 0..2 {
            let (incoming, pixels) = match transport.receive() {
                Ok(Some(value)) => value,
                Ok(None) => break,
                Err(error) => {
                    if stop_started.is_none() {
                        failure.get_or_insert(error);
                    }
                    break;
                }
            };
            if incoming.project != project.id || incoming.session != session {
                failure.get_or_insert("真实预览回执工程或会话不匹配".into());
                break;
            }
            if !ready {
                if incoming.message != "ready"
                    || incoming.revision != 0
                    || incoming.data["executable_sha256"] != executable_sha256
                {
                    failure.get_or_insert("真实预览未通过当前可执行文件SHA握手".into());
                    break;
                }
                ready = true;
                if events.try_send(Event::Ready(process.pid())).is_err() {
                    control.cancel();
                }
                continue;
            }
            match incoming.message.as_str() {
                "frame" => {
                    if let Some(expected) = &mut expected
                        && expected.accepts_frame(incoming.revision, &incoming.kind)
                    {
                        if serde_json::to_vec(&incoming.data)
                            .map_or(true, |bytes| bytes.len() > 4096)
                        {
                            failure.get_or_insert("工程运行证据超过4KiB帧预算".into());
                            break;
                        }
                        expected.last = Instant::now();
                        expected.captured = true;
                        let _ = events.try_send(Event::Frame {
                            revision: incoming.revision,
                            kind: incoming.kind,
                            sha256: Sha256::digest(&pixels)
                                .iter()
                                .map(|byte| format!("{byte:02x}"))
                                .collect(),
                            nonuniform: pixels
                                .chunks_exact(4)
                                .skip(1)
                                .any(|pixel| pixel[..3] != pixels[..3]),
                            evidence: incoming.data,
                            pixels,
                        });
                    }
                }
                "error" => {
                    if expected
                        .as_mut()
                        .is_some_and(|request| request.fail(incoming.revision, &incoming.kind))
                    {
                        error_pending = Some(Event::Error {
                            revision: incoming.revision,
                            kind: incoming.kind,
                            message: incoming.data["message"]
                                .as_str()
                                .unwrap_or("工程未提供预览错误详情")
                                .chars()
                                .take(4096)
                                .collect(),
                        });
                    }
                }
                "stopped" if stop_started.is_some() => stopped = true,
                _ => {
                    failure.get_or_insert("工程预览发送了错误方向或重复握手消息".into());
                }
            }
        }
        if let Some(event) = error_pending.take() {
            match events.try_send(event) {
                Ok(()) => (),
                Err(async_channel::TrySendError::Full(event)) => error_pending = Some(event),
                Err(async_channel::TrySendError::Closed(_)) => control.cancel(),
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
            failure.get_or_insert("工程真实预览进程意外退出".into());
            stop_started = Some(Instant::now());
        }
        if stop_started.is_some_and(|start| start.elapsed() > Duration::from_secs(2)) && !terminated
        {
            if let Err(error) = process.terminate() {
                failure.get_or_insert(error);
            }
            terminated = true;
        }
        let active = match process.active_processes() {
            Ok(active) => active,
            Err(error) => {
                failure.get_or_insert(error);
                u32::MAX
            }
        };
        if exit.is_some() && active == 0 {
            transport.shutdown();
            if transport.is_finished() && stdout.is_finished() && stderr.is_finished() {
                break;
            }
        }
        if stop_started.is_some_and(|start| start.elapsed() > Duration::from_secs(8)) {
            control.mark_cleanup_failed();
            return Err("真实预览Job、控制管道或日志线程未在8秒内收口，禁止另起进程".into());
        }
        thread::sleep(Duration::from_millis(5)); // 仅后台等待，主帧没有进程或管道等待。
    }
    let transport_result = transport.finish();
    let output = stdout.join().map_err(|_| "预览stdout线程异常")??;
    let errors = stderr.join().map_err(|_| "预览stderr线程异常")??;
    if !control.is_cancelled() {
        if let Some(error) = failure {
            return Err(format!("{error}\n{errors}\n{output}"));
        }
        transport_result?;
        if exit != Some(0) || !stopped {
            return Err(format!("工程预览异常退出：{exit:?}；{errors}"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use feathrise_runtime::project::author_preview::wire;
    use std::io::Cursor;

    fn packet(message: &str, revision: u64, kind: &str) -> Packet {
        let packet = Packet {
            project: "preview-protocol-fixture".into(),
            session: 7,
            revision,
            message: message.into(),
            kind: kind.into(),
            data: serde_json::Value::Null,
        };
        let mut bytes = Vec::new();
        let pixels = if message == "frame" {
            vec![0; wire::MAX_PIXELS]
        } else {
            Vec::new()
        };
        wire::write(&mut bytes, &packet, &pixels).unwrap();
        wire::read(&mut Cursor::new(bytes)).unwrap().unwrap().0
    }

    #[test]
    fn terminal_protocol_error_rejects_late_gpu_packets_until_a_new_request() {
        let mut expected = Expected::new(12, "rpg.skill.v1".into());
        let old = packet("error", 11, "rpg.skill.v1");
        assert!(!expected.fail(old.revision, &old.kind));
        let wrong = packet("error", 12, "rpg.character");
        assert!(!expected.fail(wrong.revision, &wrong.kind));
        let frame = packet("frame", 12, "rpg.skill.v1");
        assert!(expected.accepts_frame(frame.revision, &frame.kind));
        let error = packet("error", 12, "rpg.skill.v1");
        assert!(expected.fail(error.revision, &error.kind));
        assert!(
            !expected.accepts_frame(frame.revision, &frame.kind),
            "错误尚在UI反压队列时，迟到GPU帧也不能再次采用"
        );
        assert!(
            !expected.fail(error.revision, &error.kind),
            "终态错误只报告一次"
        );
        expected = Expected::new(13, "rpg.skill.v1".into());
        assert!(!expected.accepts_frame(frame.revision, &frame.kind));
        let new = packet("frame", 13, "rpg.skill.v1");
        assert!(expected.accepts_frame(new.revision, &new.kind));
    }
}

fn hash_file(path: &Path, control: &BuildControl) -> Result<String, String> {
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        control.check()?;
        let count = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}
fn drain_log(mut read: impl Read) -> Result<String, String> {
    let mut tail = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        let count = read.read(&mut buffer).map_err(|error| error.to_string())?;
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
fn finish_failed_start(
    process: &JobProcess,
    stdout: JoinHandle<Result<String, String>>,
    stderr: JoinHandle<Result<String, String>>,
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
    Err("预览管道初始化失败后的Job或日志线程未确认收口".into())
}
