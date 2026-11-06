//! 后台构建工具的可取消进程树：专属Job、并发日志排空、有界退出确认。
//!
//! 正常Cargo构建没有短总超时；切工程／取消／退出只终止本次Job，不能按进程名杀工具。
//! UI只持有可克隆控制牌并轮询工作槽，本模块所有等待都发生在唯一后台工作线程。

use crate::native_process::JobProcess;
#[path = "build_process_input.rs"]
mod input;
use std::{
    fs::File,
    io::Read,
    path::Path,
    process::Command,
    sync::{Arc, atomic::{AtomicBool, Ordering}, mpsc},
    thread,
    time::{Duration, Instant},
};

const POLL: Duration = Duration::from_millis(10);
const CLEANUP_LIMIT: Duration = Duration::from_secs(5);
const DIAGNOSTIC_LIMIT: usize = 16 * 1024;
const INPUT_LIMIT: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, Default)]
pub(crate) struct BuildControl(Arc<ControlState>);

/// 可选的短输入租约只读取原子代次；文件遍历与摘要仍留在后台，避免导出采用过期源码。
#[derive(Clone)]
pub(crate) struct InputGuard(pub(crate) Arc<dyn Fn() -> bool + Send + Sync>);

impl std::fmt::Debug for InputGuard {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { formatter.write_str("InputGuard") }
}

/// 后台目标注册只接受导出器已经核对归属的真实目录；回调不做目录扫描或暂停源码监听。
#[derive(Clone)]
pub(crate) struct OutputRegistrar(pub(crate) Arc<dyn Fn(&Path) -> Result<OutputLease, String> + Send + Sync>);

impl std::fmt::Debug for OutputRegistrar {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { formatter.write_str("OutputRegistrar") }
}

/// 默认释放表示取消／失败；只有完整发布回执才能保留目标以吸收迟到的原生通知。
pub(crate) struct OutputLease {
    finish: Option<Box<dyn FnOnce(Option<String>) + Send + Sync>>,
}

impl OutputLease {
    pub(crate) fn new(finish: impl FnOnce(Option<String>) + Send + Sync + 'static) -> Self {
        Self { finish: Some(Box::new(finish)) }
    }
    pub(crate) fn published(mut self, export_id: String) {
        if let Some(finish) = self.finish.take() { finish(Some(export_id)); }
    }
}

impl Drop for OutputLease {
    fn drop(&mut self) {
        if let Some(finish) = self.finish.take() { finish(None); }
    }
}

#[derive(Debug, Default)]
struct ControlState {
    cancelled: AtomicBool,
    cleanup_failed: AtomicBool,
    log: Option<crate::project_log::ProjectLogSink>,
    input_guard: Option<InputGuard>,
    output_registrar: Option<OutputRegistrar>,
    #[cfg(feature = "validation")]
    crash_requested: AtomicBool,
}

impl BuildControl {
    #[cfg(test)]
    pub(crate) fn with_input_guard(log: Option<crate::project_log::ProjectLogSink>, input_guard: Option<InputGuard>) -> Self {
        Self(Arc::new(ControlState { log, input_guard, ..Default::default() }))
    }
    pub(crate) fn with_export_guards(log: Option<crate::project_log::ProjectLogSink>, input_guard: Option<InputGuard>,
        output_registrar: Option<OutputRegistrar>) -> Self {
        Self(Arc::new(ControlState { log, input_guard, output_registrar, ..Default::default() }))
    }
    /// 仅在后台完成 destination/existing 双重校验后调用；普通构建与无监听夹具不安装注册器。
    pub(crate) fn register_output_target(&self, directory: &Path) -> Result<Option<OutputLease>, String> {
        self.check()?;
        self.0.output_registrar.as_ref().map(|registrar| (registrar.0)(directory)).transpose()
    }
    pub(crate) fn with_log(log: crate::project_log::ProjectLogSink) -> Self {
        Self(Arc::new(ControlState { log: Some(log), ..Default::default() }))
    }
    pub(crate) fn log_sink(&self) -> Option<crate::project_log::ProjectLogSink> { self.0.log.clone() }
    #[cfg(feature = "validation")]
    pub(crate) fn request_crash(&self) { self.0.crash_requested.store(true, Ordering::Release); }
    #[cfg(feature = "validation")]
    pub(crate) fn crash_requested(&self) -> bool { self.0.crash_requested.load(Ordering::Acquire) }
    pub(crate) fn cancel(&self) { self.0.cancelled.store(true, Ordering::Release); }
    pub(crate) fn is_cancelled(&self) -> bool { self.0.cancelled.load(Ordering::Acquire) }
    pub(crate) fn cleanup_failed(&self) -> bool { self.0.cleanup_failed.load(Ordering::Acquire) }
    pub(crate) fn check(&self) -> Result<(), String> {
        if self.cleanup_failed() { Err("构建工具收口未确认，禁止采用结果或启动后续工作".into()) }
        else if self.is_cancelled() { Err("构建已取消，作者文档保留".into()) }
        else if self.0.input_guard.as_ref().is_some_and(|guard| !(guard.0)()) { Err("导出期间源码或构建配置已变化，请重新导出；原输出保留".into()) }
        else { Ok(()) }
    }
    pub(crate) fn mark_cleanup_failed(&self) { self.0.cleanup_failed.store(true, Ordering::Release); }
}

#[derive(Debug)]
pub(crate) struct ProcessOutput<T> {
    pub(crate) exit_code: u32,
    pub(crate) stdout: T,
    pub(crate) stderr: String,
}

/// 在最终参数确定后记录真实构建入口；不使用Command的Debug输出，避免把环境变量整份写进日志。
/// 沿用正文解码器的有界分段，长命令不会被任务标题的单行上限静默截断。
pub(crate) fn log_build_command(command: &Command, control: &BuildControl) {
    let Some(sink) = control.log_sink() else { return; };
    let mut log = crate::project_log::LogDecoder::new(Some(sink), crate::project_log::LogSource::Build);
    if let Some(directory) = command.get_current_dir() {
        log.write(format!("工作目录：{}\n", directory.display()).as_bytes());
    }
    log.write(if cfg!(windows) { "构建命令（PowerShell）：\n& ".as_bytes() } else { "构建命令：\n".as_bytes() });
    for (index, value) in std::iter::once(command.get_program()).chain(command.get_args()).enumerate() {
        if index != 0 { log.write(b" "); }
        let text = value.to_string_lossy();
        // Windows用PowerShell单引号，Unix用shell单引号；空值、中文、空格和元字符保持参数边界。
        let plain = !text.is_empty() && text.bytes().all(|byte|
            byte.is_ascii_alphanumeric() || b"_-+=:./".contains(&byte) || cfg!(windows) && byte == b'\\');
        if plain { log.write(text.as_bytes()); }
        else {
            let quoted = text.replace('\'', if cfg!(windows) { "''" } else { "'\"'\"'" });
            log.write(format!("'{quoted}'").as_bytes());
        }
    }
    log.write(b"\n");
    log.finish();
}

/// Cargo 仍使用相同的唯一后台槽和 Job；只在真实失败后补充环境定位，不额外启动探测程序。
pub(crate) fn run_cargo<T: Send + 'static>(
    command: &Command,
    control: &BuildControl,
    timeout: Option<Duration>,
    stdout_reader: impl FnOnce(File) -> Result<T, String> + Send + 'static,
) -> Result<ProcessOutput<T>, String> {
    run(command, control, timeout, stdout_reader).map_err(|error| {
        if control.is_cancelled() || control.cleanup_failed() {
            error
        } else {
            crate::environment_diagnostics::cargo_start_failure(command, error)
        }
    })
}

/// stdout解析器必须持续排空并限制自身内存；stderr无论保留上限是否已满都继续读取。
/// 本接口保留宿主环境再应用Command覆盖；调用者不得使用Command::env_clear。
pub(crate) fn run<T: Send + 'static>(
    command: &Command,
    control: &BuildControl,
    timeout: Option<Duration>,
    stdout_reader: impl FnOnce(File) -> Result<T, String> + Send + 'static,
) -> Result<ProcessOutput<T>, String> {
    run_with_started(command, control, timeout, None, stdout_reader)
}

/// 冻结的有限输入由唯一后台写线程完整发送并关闭stdin；不经shell，也不在作者帧等待管道。
/// 结果必须同时确认写线程、两侧日志及本次Job收口，不能把启动或部分写入当作工具成功。
pub(crate) fn run_with_input<T: Send + 'static>(
    command: &Command,
    control: &BuildControl,
    timeout: Option<Duration>,
    bytes: Vec<u8>,
    stdout_reader: impl FnOnce(File) -> Result<T, String> + Send + 'static,
) -> Result<ProcessOutput<T>, String> {
    control.check()?;
    if bytes.len() > INPUT_LIMIT { return Err("构建工具stdin超过8MiB，未启动进程或截断输入".into()); }
    run_owned(command, control, timeout, None, Some(bytes), stdout_reader)
}

/// 仅在工具已创建并归属本次专属 Job 后通知调用方，不能把准备命令冒称运行版本已经生效。
pub(crate) fn run_with_started<T: Send + 'static>(
    command: &Command,
    control: &BuildControl,
    timeout: Option<Duration>,
    started: Option<mpsc::SyncSender<()>>,
    stdout_reader: impl FnOnce(File) -> Result<T, String> + Send + 'static,
) -> Result<ProcessOutput<T>, String> {
    run_owned(command, control, timeout, started, None, stdout_reader)
}

fn run_owned<T: Send + 'static>(
    command: &Command,
    control: &BuildControl,
    timeout: Option<Duration>,
    started: Option<mpsc::SyncSender<()>>,
    bytes: Option<Vec<u8>>,
    stdout_reader: impl FnOnce(File) -> Result<T, String> + Send + 'static,
) -> Result<ProcessOutput<T>, String> {
    control.check()?;
    let (process, input, output, errors) = match JobProcess::spawn_command(command) {
        Ok(spawned) => spawned,
        Err(failure) => {
            if failure.cleanup_unconfirmed { control.mark_cleanup_failed(); }
            return Err(failure.message);
        }
    };
    if let Some(started) = started { let _ = started.try_send(()); }
    let has_input = bytes.is_some();
    let (mut input_thread, input_failure) = match bytes {
        Some(bytes) => match input::InputWriter::spawn(input, bytes, control.clone()) {
            Ok(writer) => (Some(writer), None),
            Err(error) => (None, Some(error)),
        },
        None => { drop(input); (None, None) }
    };
    let (out_sender, out_receiver) = mpsc::sync_channel(1);
    let (err_sender, err_receiver) = mpsc::sync_channel(1);
    let output_thread = thread::spawn(move || { let _ = out_sender.send(stdout_reader(output)); });
    let log = control.log_sink();
    let error_thread = thread::spawn(move || {
        let reader = crate::project_log::LogReader::new(errors, log, crate::project_log::LogSource::Stderr);
        let _ = err_sender.send(read_diagnostics(reader));
    });
    let started = Instant::now();
    let mut cleanup_started = None;
    let mut failure = input_failure;
    let mut terminated = false;
    let mut exit_code = None;
    let mut stdout = None;
    let mut stderr = None;
    loop {
        if failure.is_none() {
            if control.is_cancelled() { failure = Some("构建已取消，作者文档保留".to_string()); }
            else if timeout.is_some_and(|limit| started.elapsed() >= limit) {
                failure = Some("构建工具探测超时，已请求终止专属进程树".into());
            }
        }
        if exit_code.is_none() {
            match process.try_exit() {
                Ok(code) => exit_code = code,
                Err(error) => { failure.get_or_insert(error); }
            }
        }
        receive(&out_receiver, &mut stdout, &mut failure, "构建stdout读取线程异常");
        receive(&err_receiver, &mut stderr, &mut failure, "构建stderr读取线程异常");
        if input_thread.as_ref().is_some_and(input::InputWriter::is_finished) {
            if let Err(error) = input_thread.take().unwrap().finish() { failure.get_or_insert(error); }
        }
        let active = match process.active_processes() {
            Ok(active) => active,
            Err(error) => {
                control.mark_cleanup_failed();
                // Drop仍会触发KILL_ON_JOB_CLOSE，但不能将其冒称为已确认退出。
                let error = format!("{error}；无法确认构建子树已退出");
                if !has_input { return Err(error); }
                failure.get_or_insert(error);
                u32::MAX // stdin变体仍持有精确Job与全部线程，继续尝试终止和实际回收。
            }
        };
        if failure.is_some() || exit_code.is_some() {
            let cleanup_start = *cleanup_started.get_or_insert_with(Instant::now);
            if let Some(writer) = &input_thread { writer.cancel(); }
            // 根工具退出也不允许遗留build.rs孙进程持有管道；只处理精确拥有的Job。
            if active != 0 && !terminated {
                match process.terminate() {
                    Ok(()) => terminated = true,
                    Err(error) => { failure.get_or_insert(error); }
                }
            }
            if active == 0 && exit_code.is_some() && input_thread.is_none()
                && output_thread.is_finished() && error_thread.is_finished() {
                // is_finished为真后join不会等待工具或日志；此处仅取回线程panic状态。
                let output_joined = output_thread.join().is_ok();
                let errors_joined = error_thread.join().is_ok();
                receive(&out_receiver, &mut stdout, &mut failure, "构建stdout读取线程异常");
                receive(&err_receiver, &mut stderr, &mut failure, "构建stderr读取线程异常");
                if !output_joined || !errors_joined { failure.get_or_insert("构建日志读取线程异常".into()); }
                if let Some(failure) = failure { return Err(failure); }
                control.check()?;
                return Ok(ProcessOutput {
                    exit_code: exit_code.expect("完成前已确认根工具退出"),
                    stdout: stdout.ok_or("构建stdout结果缺失")??,
                    stderr: stderr.ok_or("构建stderr结果缺失")??,
                });
            }
            if cleanup_start.elapsed() >= CLEANUP_LIMIT {
                control.mark_cleanup_failed();
                if has_input {
                    // 操作系统取消只是请求，不能在writer仍阻塞时丢句柄、归还槽或遗留后台线程。
                    // 超预算先锁存失败，仍由当前后台槽持有；普通无stdin入口保留原五秒返回规则。
                    if input_thread.is_some() || !output_thread.is_finished() || !error_thread.is_finished() {
                        thread::sleep(POLL);
                        continue;
                    }
                    let _ = output_thread.join();
                    let _ = error_thread.join();
                }
                return Err(format!("{}；5秒内未确认构建子树及日志线程收口（剩余进程{active}）",
                    failure.unwrap_or_else(|| "构建根工具已退出".into())));
            }
        }
        thread::sleep(POLL);
    }
}

fn receive<T>(receiver: &mpsc::Receiver<Result<T, String>>, slot: &mut Option<Result<T, String>>,
    failure: &mut Option<String>, disconnected: &str) {
    if slot.is_some() { return; }
    match receiver.try_recv() {
        Ok(value) => {
            if let Err(error) = &value { failure.get_or_insert(error.clone()); }
            *slot = Some(value);
        }
        Err(mpsc::TryRecvError::Empty) => {}
        Err(mpsc::TryRecvError::Disconnected) => { failure.get_or_insert(disconnected.into()); }
    }
}

fn read_diagnostics(mut reader: impl Read) -> Result<String, String> {
    let mut tail = Vec::with_capacity(DIAGNOSTIC_LIMIT);
    let mut buffer = [0u8; 8192];
    loop {
        let size = reader.read(&mut buffer).map_err(|e| format!("构建诊断读取失败：{e}"))?;
        if size == 0 { break; }
        let excess = (tail.len() + size).saturating_sub(DIAGNOSTIC_LIMIT);
        if excess != 0 { tail.drain(..excess); }
        tail.extend_from_slice(&buffer[..size]);
    }
    Ok(String::from_utf8_lossy(&tail).into_owned())
}

// 本组检查Win32句柄继承与精确Job收口；Unix后端另有自己的进程组/管道回归。
#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader, Write},
        os::windows::{io::{AsRawHandle, FromRawHandle, OwnedHandle}, process::CommandExt},
        process::Stdio,
        sync::Mutex,
    };
    use windows_sys::Win32::{
        Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT},
        System::Threading::{CREATE_NO_WINDOW, CREATE_SUSPENDED, OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject},
    };

    const MODE: &str = "EDITOR_OWNED_BUILD_PROCESS_TEST_7ACD";

    fn helper_command(mode: &str) -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command.args(["--ignored", "--exact", "build_process::tests::tool_child_helper",
            "--nocapture", "--test-threads=1"]);
        command.env(MODE, mode);
        command
    }

    fn all_output(mut file: File) -> Result<Vec<u8>, String> {
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
        Ok(bytes)
    }

    fn process_handle(pid: u32) -> Result<OwnedHandle, String> {
        let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
        if handle.is_null() { return Err(format!("取消前无法取得被测进程{pid}句柄：{}", std::io::Error::last_os_error())); }
        Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
    }

    fn await_empty(process: &JobProcess) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while process.active_processes().unwrap() != 0 || process.try_exit().unwrap().is_none() {
            assert!(Instant::now() < deadline, "自有对照Job未收口");
            thread::sleep(POLL);
        }
    }

    #[test]
    fn cancelled_control_rejects_before_starting_any_program() {
        let control = BuildControl::default();
        control.cancel();
        let error = run_cargo(&Command::new("must-not-be-created.exe"), &control, None, all_output).unwrap_err();
        assert!(error.contains("取消"));
        assert!(!error.contains("cargo --version"));
        assert!(!control.cleanup_failed());
    }

    #[test]
    fn missing_tool_is_retryable_without_marking_cleanup_unconfirmed() {
        let control = BuildControl::default();
        let missing = std::env::temp_dir().join(format!("editor-missing-owned-tool-{}.exe", std::process::id()));
        let error = run_cargo(&Command::new(missing), &control, None, all_output).unwrap_err();
        assert!(error.contains("cargo --version"));
        assert!(!control.cleanup_failed());
        control.check().unwrap();
        // 同一控制牌仍可启动正确工具；普通路径错误不能误升级成永久收口屏障。
        let output = run(&helper_command("orphan"), &control, None, all_output).unwrap();
        assert_eq!(output.exit_code, 0);
        assert!(!control.cleanup_failed());
    }

    #[test]
    fn tool_process_drains_both_large_streams_and_preserves_unicode_environment_and_cwd() {
        let root = std::env::temp_dir().join(format!("editor-build-中文-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let mut command = helper_command("success");
        command.current_dir(&root).env("EDITOR_BUILD_TEST_UNICODE", "中文 环境");
        let control = BuildControl::default();
        let result = run(&command, &control, None, all_output).unwrap();
        assert_eq!(result.exit_code, 37);
        let output = String::from_utf8(result.stdout).unwrap();
        assert!(output.contains("ENV:中文 环境"));
        assert!(output.contains(&format!("CWD:{}", root.display())));
        assert!(output.contains(&"o".repeat(128 * 1024)));
        assert!(result.stderr.ends_with("STDERR_TAIL\n"));
        assert!(result.stderr.len() <= DIAGNOSTIC_LIMIT + 3);
        assert!(!control.cleanup_failed());
        std::fs::remove_dir(root).unwrap();
    }

    #[test]
    fn cancellation_reclaims_owned_hung_grandchild_without_touching_sibling() {
        let sibling_command = helper_command("sleep");
        let (sibling, _input, _output, _errors) = JobProcess::spawn_command(&sibling_command).unwrap();
        let control = BuildControl::default();
        let worker_control = control.clone();
        let (ready_sender, ready_receiver) = mpsc::channel();
        let (result_sender, result_receiver) = mpsc::sync_channel(1);
        let diagnostics = Arc::new(Mutex::new(Vec::new()));
        let observed_diagnostics = diagnostics.clone();
        let worker = thread::spawn(move || {
            let result = run(&helper_command("tree"), &worker_control, None, move |file| {
                for line in BufReader::new(file).lines() {
                    let line = line.map_err(|e| e.to_string())?;
                    let mut diagnostic = observed_diagnostics.lock().unwrap();
                    if diagnostic.len() == 16 { diagnostic.remove(0); }
                    diagnostic.push(line.chars().take(512).collect::<String>());
                    drop(diagnostic);
                    if let Some((_, pids)) = line.split_once("OWNED_TREE:") {
                        let values = pids.split(',').map(|part| part.parse::<u32>().map_err(|e| e.to_string()))
                            .collect::<Result<Vec<_>, _>>()?;
                        let _ = ready_sender.send(values);
                    }
                }
                Ok(())
            });
            let _ = result_sender.send(result);
        });
        let observation = (|| -> Result<Vec<OwnedHandle>, String> {
            let pids = ready_receiver.recv_timeout(Duration::from_secs(5)).map_err(|e| format!("等待OWNED_TREE就绪失败：{e}"))?;
            if pids.len() != 2 { return Err(format!("根/孙进程数量无效：{pids:?}")); }
            let handles = pids.into_iter().map(process_handle).collect::<Result<Vec<_>, _>>()?;
            for handle in &handles {
                if unsafe { WaitForSingleObject(handle.as_raw_handle(), 0) } != WAIT_TIMEOUT {
                    return Err("取消前被测根或挂起孙进程已退出".into());
                }
            }
            Ok(handles)
        })();
        // 就绪超时、协议错误和句柄检查失败也必须先取消、收口；不能unwrap后留下后台树。
        let started = Instant::now();
        control.cancel();
        let result = result_receiver.recv_timeout(Duration::from_secs(6));
        let worker_deadline = Instant::now() + Duration::from_secs(1);
        while !worker.is_finished() && Instant::now() < worker_deadline { thread::sleep(Duration::from_millis(1)); }
        let worker_finished = worker.is_finished();
        let worker_joined = if worker_finished { worker.join().is_ok() } else { false };
        let sibling_alive = sibling.try_exit();
        let sibling_terminated = sibling.terminate();
        if sibling_terminated.is_ok() { await_empty(&sibling); }
        let handles = observation.unwrap_or_else(|e| panic!("{e}；stdout={:?}；run结果={result:?}", diagnostics.lock().unwrap()));
        assert!(result.expect("取消后后台run未在6秒内返回").unwrap_err().contains("取消"));
        assert!(worker_finished, "run返回后工作线程未结束");
        assert!(worker_joined, "run工作线程异常");
        assert!(started.elapsed() < Duration::from_secs(6));
        assert!(!control.cleanup_failed());
        for handle in handles {
            assert_eq!(unsafe { WaitForSingleObject(handle.as_raw_handle(), 0) }, WAIT_OBJECT_0);
        }
        assert_eq!(sibling_alive.unwrap(), None);
        sibling_terminated.unwrap();
    }

    #[test]
    fn tool_probe_timeout_stops_owned_process_and_reader_failure_does_not_wait_for_exit() {
        let control = BuildControl::default();
        let started = Instant::now();
        let error = run_cargo(&helper_command("sleep"), &control, Some(Duration::from_millis(150)), all_output).unwrap_err();
        assert!(error.contains("超时"));
        assert!(!error.contains("cargo --version"));
        assert!(started.elapsed() < Duration::from_secs(6));
        assert!(!control.cleanup_failed());
        let error = run_cargo(&helper_command("sleep"), &control, None, |_| Err::<(), _>("故意拒绝解析".into())).unwrap_err();
        assert!(error.contains("故意拒绝解析"));
        assert!(!error.contains("cargo --version"));
        assert!(!control.cleanup_failed());
    }

    #[test]
    fn successful_root_exit_closes_orphan_descendant_pipe_writers() {
        let control = BuildControl::default();
        let result = run(&helper_command("orphan"), &control, None, all_output).unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(String::from_utf8(result.stdout).unwrap().contains("OWNED_TREE:"));
        assert!(!control.cleanup_failed());
    }

    #[test]
    fn stdin_limit_rejects_before_creating_a_tool_or_truncating_bytes() {
        let control = BuildControl::default();
        let error = run_with_input(&Command::new("must-not-be-created.exe"), &control, None,
            vec![0; INPUT_LIMIT + 1], all_output).unwrap_err();
        assert!(error.contains("stdin超过8MiB"));
        assert!(!control.cleanup_failed());
    }

    #[test]
    fn stdin_delivers_exact_limit_and_eof_while_both_output_pipes_are_full() {
        for bytes in [Vec::new(), (0..INPUT_LIMIT).map(|index| (index % 251) as u8).collect::<Vec<_>>()] {
            let expected = bytes.clone();
            let control = BuildControl::default();
            let result = run_with_input(&helper_command("stdin_echo"), &control,
                Some(Duration::from_secs(10)), bytes, all_output).unwrap();
            assert_eq!(result.exit_code, 0);
            let marker = b"\nOWNED_STDIN_BEGIN\n";
            let start = result.stdout.windows(marker.len()).position(|window| window == marker).unwrap() + marker.len();
            assert_eq!(&result.stdout[start..start + expected.len()], expected.as_slice());
            assert!(result.stdout[start + expected.len()..].starts_with(b"\nOWNED_STDIN_END\n"));
            assert!(result.stderr.ends_with("STDIN_STDERR_TAIL\n"));
            assert!(result.stderr.len() <= DIAGNOSTIC_LIMIT + 3);
            assert!(!control.cleanup_failed());
        }
    }

    #[test]
    fn stdin_backpressure_cancel_reaps_the_owned_writer_and_process() {
        let control = BuildControl::default();
        let worker_control = control.clone();
        let (ready, observed) = mpsc::sync_channel(1);
        let (completed, result) = mpsc::sync_channel(1);
        let worker = thread::spawn(move || {
            let output = run_with_input(&helper_command("stdin_stall"), &worker_control, None,
                vec![b'x'; INPUT_LIMIT], move |file| {
                    for line in BufReader::new(file).lines() {
                        let line = line.map_err(|error| error.to_string())?;
                        if let Some(pid) = line.strip_prefix("OWNED_STDIN_READY:") {
                            let _ = ready.try_send(pid.parse::<u32>().map_err(|error| error.to_string())?);
                        }
                    }
                    Ok(())
                });
            let _ = completed.send(output);
        });
        let observation = observed.recv_timeout(Duration::from_secs(5))
            .map_err(|error| error.to_string()).and_then(process_handle);
        // 子进程不读取stdin，输入远大于匿名管道容量；取消必须解除反压，不能遗留独立写线程。
        let started = Instant::now();
        control.cancel();
        let output = result.recv_timeout(Duration::from_secs(6));
        let deadline = Instant::now() + Duration::from_secs(1);
        while !worker.is_finished() && Instant::now() < deadline { thread::sleep(POLL); }
        let finished = worker.is_finished();
        if finished { worker.join().unwrap(); }
        let handle = observation.expect("自有不读stdin进程必须先实际就绪");
        assert!(output.expect("stdin反压取消后协调器未退出").unwrap_err().contains("取消"));
        assert!(finished, "协调器返回后仍遗留写入/日志工作线程");
        assert!(started.elapsed() < Duration::from_secs(6));
        assert_eq!(unsafe { WaitForSingleObject(handle.as_raw_handle(), 0) }, WAIT_OBJECT_0);
        assert!(!control.cleanup_failed());
    }

    #[test]
    fn stdin_timeout_and_early_closed_pipe_finish_before_the_control_is_reused() {
        let control = BuildControl::default();
        let started = Instant::now();
        let error = run_with_input(&helper_command("stdin_stall"), &control, Some(Duration::from_millis(150)),
            vec![b'x'; INPUT_LIMIT], all_output).unwrap_err();
        assert!(error.contains("超时"));
        assert!(started.elapsed() < Duration::from_secs(6));
        assert!(!control.cleanup_failed());
        let error = run_with_input(&helper_command("stdin_closed"), &control, Some(Duration::from_secs(5)),
            vec![b'x'; INPUT_LIMIT], all_output).unwrap_err();
        assert!(error.contains("stdin"));
        assert!(!control.cleanup_failed());
        let result = run_with_input(&helper_command("stdin_echo"), &control, Some(Duration::from_secs(5)),
            "再次使用原控制牌".as_bytes().to_vec(), all_output).unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(!control.cleanup_failed());
    }

    #[test]
    #[ignore = "仅供本模块通过专属Job创建的自有工具进程调用"]
    fn tool_child_helper() {
        let Ok(mode) = std::env::var(MODE) else { return; };
        match mode.as_str() {
            "stdin_echo" => {
                // 先填满两侧输出，再读输入至EOF；父侧若串行写stdin或不关写端，此夹具会死锁。
                println!("{}", "o".repeat(128 * 1024));
                eprint!("{}", "e".repeat(128 * 1024));
                let mut bytes = Vec::new();
                std::io::stdin().read_to_end(&mut bytes).unwrap();
                let mut stdout = std::io::stdout().lock();
                stdout.write_all(b"\nOWNED_STDIN_BEGIN\n").unwrap();
                stdout.write_all(&bytes).unwrap();
                stdout.write_all(b"\nOWNED_STDIN_END\n").unwrap();
                stdout.flush().unwrap();
                eprintln!("STDIN_STDERR_TAIL");
                std::process::exit(0);
            }
            "stdin_stall" => {
                println!("\nOWNED_STDIN_READY:{}", std::process::id());
                std::io::stdout().flush().unwrap();
            }
            "stdin_closed" => std::process::exit(0),
            "success" => {
                println!("ENV:{}", std::env::var("EDITOR_BUILD_TEST_UNICODE").unwrap());
                println!("CWD:{}", std::env::current_dir().unwrap().canonicalize().unwrap().display());
                // 两侧均超过匿名管道容量，验证父侧持续并发排空且诊断只保留固定尾部。
                println!("{}", "o".repeat(128 * 1024));
                eprint!("{}", "e".repeat(128 * 1024));
                eprintln!("STDERR_TAIL");
                std::process::exit(37);
            }
            "tree" | "orphan" => {
                let mut child = helper_command("sleep");
                // 刻意创建OS挂起孙进程，不依赖其合作退出；它仍持有继承的日志写端。
                child.stdin(Stdio::null()).stdout(Stdio::inherit()).stderr(Stdio::inherit())
                    .creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED);
                let child = child.spawn().unwrap();
                // libtest会先打印不换行的“test ...”，标记须自占一行便于父端解析。
                println!("\nOWNED_TREE:{},{}", std::process::id(), child.id());
                std::io::stdout().flush().unwrap();
                if mode == "orphan" { std::process::exit(0); }
            }
            "sleep" => {}
            _ => panic!("未知测试模式"),
        }
        loop { thread::sleep(Duration::from_secs(60)); }
    }
}
