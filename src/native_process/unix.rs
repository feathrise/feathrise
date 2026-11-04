//! Unix 原生进程后端：独占进程组、close-on-exec 管道和精确句柄收口；不经 shell 或进程名扫描。

use std::{
    ffi::{OsStr, OsString},
    fs::File,
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::{fs::PermissionsExt, process::CommandExt},
    },
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::Mutex,
};

#[derive(Debug)]
pub(crate) struct SpawnFailure {
    pub(crate) message: String,
    pub(crate) cleanup_unconfirmed: bool,
}

impl From<String> for SpawnFailure {
    fn from(message: String) -> Self {
        Self {
            message,
            cleanup_unconfirmed: false,
        }
    }
}
impl From<&str> for SpawnFailure {
    fn from(message: &str) -> Self {
        message.to_owned().into()
    }
}

/// 根进程在 exec 前建立独立组；所有停止都针对本次拥有的组，不接触宿主所属组。
pub struct JobProcess {
    child: Mutex<ProcessState>,
    pid: u32,
    // 根进程退出后仍需排空同组后代；确认组为空后永久撤销信号权限，避免 PGID 复用。
    drained: std::sync::atomic::AtomicBool,
}

struct ProcessState {
    child: Option<Child>,
    exit: Option<u32>,
    signaled: bool,
}

impl JobProcess {
    pub fn spawn(exe: &Path, args: &[OsString]) -> Result<(Self, File, File, File), String> {
        Self::spawn_command(&absolute_command(exe, args)?).map_err(|failure| failure.message)
    }

    pub(crate) fn spawn_command(
        command: &Command,
    ) -> Result<(Self, File, File, File), SpawnFailure> {
        let (process, input, output, errors, _) = spawn_inner(command, false)?;
        Ok((process, input, output, errors))
    }

    pub fn spawn_runtime(
        exe: &Path,
        args: &[OsString],
    ) -> Result<(Self, File, File, File, File), String> {
        Self::spawn_runtime_command(&absolute_command(exe, args)?)
            .map_err(|failure| failure.message)
    }

    pub(crate) fn spawn_runtime_command(
        command: &Command,
    ) -> Result<(Self, File, File, File, File), SpawnFailure> {
        if command
            .get_args()
            .any(|argument| argument == "--ipc-read" || argument == "--ipc-write")
        {
            return Err("协议管道描述符只能由受控进程边界生成".into());
        }
        let (process, input, output, errors, control) = spawn_inner(command, true)?;
        drop(input);
        let (write, read) = control.expect("请求了独立控制通道");
        Ok((process, write, read, output, errors))
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    pub fn try_exit(&self) -> Result<Option<u32>, String> {
        let mut state = self
            .child
            .lock()
            .map_err(|_| "原生子进程锁已损坏".to_owned())?;
        if state.exit.is_none() {
            let mut information: libc::siginfo_t = unsafe { std::mem::zeroed() };
            // WNOWAIT先观察退出但保留直属子进程的PID占有；发完唯一一次组信号前不得reap，
            // 否则PGID可能被系统复用，旧任务的取消就可能误伤另一次任务。
            if unsafe {
                libc::waitid(
                    libc::P_PID,
                    self.pid as libc::id_t,
                    &mut information,
                    libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                )
            } != 0
            {
                return Err(format!(
                    "无法查询原生子进程：{}",
                    std::io::Error::last_os_error()
                ));
            }
            if unsafe { information.si_pid() } != 0 {
                let code = unsafe { information.si_status() };
                state.exit = Some(if information.si_code == libc::CLD_EXITED {
                    code as u32
                } else {
                    (128 + code) as u32
                });
            }
        }
        if state.signaled && state.exit.is_some() {
            // 已永久撤销本次组信号权限后才回收直属子进程；后续仅做无副作用的归零查询。
            if let Some(child) = state.child.as_mut() {
                child.try_wait().map_err(|error| error.to_string())?;
            }
        }
        Ok(state.exit)
    }

    pub fn terminate(&self) -> Result<(), String> {
        let mut state = self
            .child
            .lock()
            .map_err(|_| "原生子进程锁已损坏".to_owned())?;
        if state.signaled {
            return Ok(());
        }
        let result = unsafe { libc::kill(-(self.pid as libc::pid_t), libc::SIGKILL) };
        let error = std::io::Error::last_os_error();
        if result != 0 && error.raw_os_error() != Some(libc::ESRCH) {
            return Err(format!("无法结束本次原生进程组：{error}"));
        }
        // 根若主动改变组，仍精确结束这个尚未reap的直属Child；不能按名称追踪已逃逸其他会话。
        if let Some(child) = state.child.as_mut() {
            if let Err(error) = child.kill() {
                if error.raw_os_error() != Some(libc::ESRCH) {
                    return Err(format!("无法结束本次原生根进程：{error}"));
                }
            }
        }
        state.signaled = true;
        Ok(())
    }

    /// Unix 无 Job 计数 API；返回 1 表示组仍存活，0 才是完成，不能把根退出冒充后代已收口。
    pub fn active_processes(&self) -> Result<u32, String> {
        use std::sync::atomic::Ordering;
        if self.drained.load(Ordering::Acquire) {
            return Ok(0);
        }
        let _ = self.try_exit()?;
        if unsafe { libc::kill(-(self.pid as libc::pid_t), 0) } == 0 {
            return Ok(1);
        }
        let error = std::io::Error::last_os_error();
        match error.raw_os_error() {
            Some(libc::ESRCH) => {
                self.drained.store(true, Ordering::Release);
                Ok(0)
            }
            Some(libc::EPERM) => Ok(1),
            _ => Err(format!("无法查询本次原生进程组：{error}")),
        }
    }
}

impl Drop for JobProcess {
    fn drop(&mut self) {
        // 发出回收保障但不在 UI 线程 wait；调用者仍须按 active_processes 确认正式收口。
        let _ = self.terminate();
        let state = self
            .child
            .get_mut()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(mut child) = state.child.take() {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
    }
}

fn absolute_command(exe: &Path, args: &[OsString]) -> Result<Command, String> {
    if !exe.is_absolute() {
        return Err("子进程启动必须提供可执行文件的绝对路径".into());
    }
    let mut command = Command::new(exe);
    command.args(args);
    Ok(command)
}

fn executable(command: &Command) -> Result<PathBuf, String> {
    let program = Path::new(command.get_program());
    let candidates = if program.is_absolute() {
        vec![program.to_owned()]
    } else if program.components().count() > 1 {
        vec![
            command
                .get_current_dir()
                .map(Path::to_owned)
                .unwrap_or(std::env::current_dir().map_err(|error| error.to_string())?)
                .join(program),
        ]
    } else {
        let path = match command
            .get_envs()
            .find(|(key, _)| *key == OsStr::new("PATH"))
        {
            Some((_, value)) => value.map(OsStr::to_owned),
            None => std::env::var_os("PATH"),
        }
        .ok_or("工具环境缺少 PATH，必须提供可执行文件绝对路径")?;
        std::env::split_paths(&path)
            .filter(|directory| directory.is_absolute())
            .map(|directory| directory.join(program))
            .collect()
    };
    for path in candidates {
        if let Ok(path) = path.canonicalize() {
            if let Ok(metadata) = path.metadata() {
                if metadata.is_file() && metadata.permissions().mode() & 0o111 != 0 {
                    return Ok(path);
                }
            }
        }
    }
    Err(format!("找不到可执行的原生工具：{}", program.display()))
}

type Spawned = (JobProcess, File, File, File, Option<(File, File)>);

fn spawn_inner(options: &Command, dedicated: bool) -> Result<Spawned, SpawnFailure> {
    let mut command = Command::new(executable(options)?);
    command.args(options.get_args()).process_group(0);
    if let Some(directory) = options.get_current_dir() {
        command.current_dir(directory);
    }
    for (key, value) in options.get_envs() {
        if let Some(value) = value {
            command.env(key, value);
        } else {
            command.env_remove(key);
        }
    }
    let (child_input, parent_input) = pipe()?;
    let (parent_output, child_output) = pipe()?;
    let (parent_errors, child_errors) = pipe()?;
    command
        .stdin(Stdio::from(child_input))
        .stdout(Stdio::from(child_output))
        .stderr(Stdio::from(child_errors));
    let control = if dedicated {
        Some((pipe()?, pipe()?))
    } else {
        None
    };
    if let Some(((child_read, _), (_, child_write))) = &control {
        let read = child_read.as_raw_fd();
        let write = child_write.as_raw_fd();
        command.args([
            "--ipc-read",
            &read.to_string(),
            "--ipc-write",
            &write.to_string(),
        ]);
        // SAFETY：fork 后仅执行 async-signal-safe fcntl；只打开本次两端，其他端仍 CLOEXEC。
        unsafe {
            command.pre_exec(move || {
                for descriptor in [read, write] {
                    if libc::fcntl(descriptor, libc::F_SETFD, 0) < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                }
                Ok(())
            });
        }
    }
    let child = command
        .spawn()
        .map_err(|error| format!("无法启动原生工具：{error}"))?;
    let pid = child.id();
    Ok((
        JobProcess {
            child: Mutex::new(ProcessState {
                child: Some(child),
                exit: None,
                signaled: false,
            }),
            pid,
            drained: false.into(),
        },
        File::from(parent_input),
        File::from(parent_output),
        File::from(parent_errors),
        control.map(|((_, write), (read, _))| (File::from(write), File::from(read))),
    ))
}

fn pipe() -> Result<(OwnedFd, OwnedFd), String> {
    let mut descriptors = [-1; 2];
    #[cfg(target_os = "linux")]
    let result = unsafe { libc::pipe2(descriptors.as_mut_ptr(), libc::O_CLOEXEC) };
    #[cfg(not(target_os = "linux"))]
    let result = unsafe { libc::pipe(descriptors.as_mut_ptr()) };
    if result != 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let read = unsafe { OwnedFd::from_raw_fd(descriptors[0]) };
    let write = unsafe { OwnedFd::from_raw_fd(descriptors[1]) };
    let read = nonstandard_descriptor(read)?;
    let write = nonstandard_descriptor(write)?;
    for descriptor in [&read, &write] {
        if unsafe { libc::fcntl(descriptor.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
    }
    Ok((read, write))
}

fn nonstandard_descriptor(descriptor: OwnedFd) -> Result<OwnedFd, String> {
    if descriptor.as_raw_fd() >= 3 {
        return Ok(descriptor);
    }
    // 从桌面启动时标准流也可能未打开；不能让专用控制FD占用0/1/2再被Stdio重定向覆盖。
    let duplicate = unsafe { libc::fcntl(descriptor.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 3) };
    if duplicate < 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(unsafe { OwnedFd::from_raw_fd(duplicate) })
}

/// 接管宿主明确传入的两个管道 FD，并阻止下一次 exec 继续继承。
///
/// # Safety
/// 两个值必须是本次 spawn_runtime 唯一传入且仍存活的不同管道子端；只能接管一次。
pub unsafe fn child_channels(read: usize, write: usize) -> Result<(File, File), String> {
    if read < 3
        || write < 3
        || read > i32::MAX as usize
        || write > i32::MAX as usize
        || read == write
    {
        return Err("专用控制管道需要两个有效且不同的非标准描述符".into());
    }
    let read = unsafe { OwnedFd::from_raw_fd(read as i32) };
    let write = unsafe { OwnedFd::from_raw_fd(write as i32) };
    for descriptor in [&read, &write] {
        if unsafe { libc::fcntl(descriptor.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
    }
    Ok((File::from(read), File::from(write)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn owned_group_is_signaled_before_reap_and_reports_drain() {
        let command = Command::new("true");
        let (process, input, output, errors) = JobProcess::spawn_command(&command).unwrap();
        drop((input, output, errors));
        let since = Instant::now();
        while process.try_exit().unwrap().is_none() {
            assert!(since.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(process.try_exit().unwrap(), Some(0));
        assert_eq!(
            process.active_processes().unwrap(),
            1,
            "尚未发停止信号前保留已退出根的PID占有"
        );
        process.terminate().unwrap();
        while process.active_processes().unwrap() != 0 {
            assert!(since.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(5));
        }
        process.terminate().unwrap(); // 归零后不再对可复用的PGID发信号。
    }

    #[test]
    fn pipe_descriptors_are_nonstandard_and_close_on_exec() {
        let (read, write) = pipe().unwrap();
        for descriptor in [&read, &write] {
            assert!(descriptor.as_raw_fd() >= 3);
            assert_ne!(
                unsafe { libc::fcntl(descriptor.as_raw_fd(), libc::F_GETFD) } & libc::FD_CLOEXEC,
                0
            );
        }
    }
}
