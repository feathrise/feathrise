//! Linux／macOS原生选择器：短生命周期辅助进程承载阻塞系统对话框，作者帧只轮询。

use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::{
    io::{Read, Write},
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

pub(crate) struct FolderPicker {
    receiver: Receiver<Result<Option<String>, String>>,
    worker: Option<JoinHandle<()>>,
    cancel: Arc<AtomicBool>,
}

impl FolderPicker {
    pub fn start(owner: usize, initial: PathBuf, new: bool) -> Result<Self, String> {
        Self::spawn(owner, initial, if new { "new" } else { "directory" })
    }

    pub fn start_plugin(owner: usize, initial: PathBuf) -> Result<Self, String> {
        Self::spawn(owner, initial, "plugin")
    }

    /// 只增加独立用途，不增加另一个进程所有者；导出也沿用取消优先及有界结果轮询。
    pub(crate) fn start_export(owner: usize, initial: PathBuf) -> Result<Self, String> {
        Self::spawn(owner, initial, "export")
    }

    fn spawn(owner: usize, initial: PathBuf, kind: &'static str) -> Result<Self, String> {
        let (sender, receiver) = mpsc::sync_channel(1);
        let cancel = Arc::new(AtomicBool::new(false));
        let cancelled = cancel.clone();
        let worker = thread::Builder::new()
            .name("editor-native-picker".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(|| choose(owner, initial, kind, &cancelled))
                    .unwrap_or_else(|_| Err("系统选择器异常，请重试。".into()));
                let _ = sender.send(result);
            })
            .map_err(|error| format!("无法启动系统选择器：{error}"))?;
        Ok(Self {
            receiver,
            worker: Some(worker),
            cancel,
        })
    }

    pub fn poll(&mut self) -> Option<Result<Option<String>, String>> {
        if !self.worker.as_ref()?.is_finished() {
            return None;
        }
        let _ = self.worker.take()?.join();
        let result = self
            .receiver
            .try_recv()
            .unwrap_or_else(|_| Err("系统选择器未返回结果。".into()));
        // 关闭授权可能晚于helper结果入队；取消优先，不能在退出收口时反而登记新安装。
        Some(if self.cancel.load(Ordering::Acquire) {
            Ok(None)
        } else {
            result
        })
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
    }
}

impl Drop for FolderPicker {
    fn drop(&mut self) {
        // 不在主帧join阻塞的系统窗口；工作线程观察取消后只回收自己创建的进程组。
        self.cancel();
    }
}

fn choose(
    owner: usize,
    initial: PathBuf,
    kind: &str,
    cancel: &AtomicBool,
) -> Result<Option<String>, String> {
    if cancel.load(Ordering::Acquire) {
        return Ok(None);
    }
    let mut command = Command::new(std::env::current_exe().map_err(|error| error.to_string())?);
    command
        .args(["--native-picker", kind])
        .arg(initial)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0);
    // X11窗口ID可跨进程交给portal；Wayland的指针不能伪装成可导出窗口句柄。
    if let Some(parent) = std::env::var_os("FEATHRISE_WORKSPACE_PARENT_WINDOW") {
        command.env("FEATHRISE_PICKER_PARENT_WINDOW", parent);
    } else if cfg!(target_os = "linux") && owner != 0 {
        command.env("FEATHRISE_PICKER_PARENT_WINDOW", format!("x11:{owner}"));
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("无法打开系统选择器：{error}"))?;
    // 单独保留stdin写端。父宿主异常退出也会关闭它，辅助进程的EOF看门线程随即退出。
    let _lifetime = child.stdin.take();
    let mut stdout = child.stdout.take().ok_or("选择器缺少结果通道")?;
    let fd = stdout.as_raw_fd();
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags == -1 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1 {
        let error = std::io::Error::last_os_error();
        let _ = child.kill();
        let _ = child.wait();
        return Err(format!("无法建立选择器非阻塞结果通道：{error}"));
    }
    let mut bytes = Vec::new();
    let mut status = None;
    let mut exited_at = None;
    let mut eof = false;
    loop {
        if cancel.load(Ordering::Acquire) {
            // 未回收leader时仍拥有PID/PGID，才可取消本次组；回收后绝不凭旧PID发送组信号。
            if status.is_none() {
                unsafe {
                    libc::kill(-(child.id() as i32), libc::SIGKILL);
                }
                let _ = child.wait();
            }
            return Ok(None);
        }
        // 与进程退出同时轮询pipe，不join可能被后代保留写端的阻塞读取线程。
        let mut block = [0; 4096];
        match stdout.read(&mut block) {
            Ok(0) => eof = true,
            Ok(count) => bytes.extend_from_slice(&block[..count]),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                ) => {}
            Err(error) => {
                if status.is_none() {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                return Err(format!("无法读取系统选择结果：{error}"));
            }
        }
        if bytes.len() > 16 * 1024 {
            if status.is_none() {
                let _ = child.kill();
                let _ = child.wait();
            }
            return Err("选择的路径超过长度预算。".into());
        }
        if status.is_none() {
            match child.try_wait() {
                Ok(Some(result)) => {
                    status = Some(result);
                    exited_at = Some(std::time::Instant::now());
                }
                Ok(None) => {}
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("无法等待系统选择器：{error}"));
                }
            }
        }
        if let Some(status) = status {
            if !status.success() {
                return Err("系统选择器未正常完成，请检查桌面原生对话框支持后重试。".into());
            }
            if eof {
                return serde_json::from_slice(&bytes)
                    .map_err(|error| format!("系统选择结果无效：{error}"));
            }
            if exited_at.is_some_and(|at| at.elapsed() >= Duration::from_secs(2)) {
                return Err("系统选择器已退出，但结果通道未关闭。".into());
            }
        }
        thread::sleep(Duration::from_millis(20));
    }
}

/// 仅在应用入口、尚未创建任何Bevy窗口时调用，保证macOS原生面板在辅助进程主线程运行。
pub(crate) fn run_helper() -> Result<bool, String> {
    let mut arguments = std::env::args_os().skip(1);
    if arguments.next().as_deref() != Some(std::ffi::OsStr::new("--native-picker")) {
        return Ok(false);
    }
    let kind = arguments.next().ok_or("选择器缺少用途")?;
    let initial = PathBuf::from(arguments.next().ok_or("选择器缺少初始目录")?);
    if arguments.next().is_some() {
        return Err("选择器参数过多".into());
    }
    let title = match kind.to_str() {
        Some("plugin") => "选择插件文件",
        Some("directory") => "选择已有工程目录",
        Some("new") => "选择新工程的父目录",
        Some("export") => "选择工程导出目录",
        _ => return Err("未知系统选择用途".into()),
    };
    thread::Builder::new()
        .name("picker-parent-lifetime".into())
        .spawn(|| {
            let mut byte = [0; 1];
            // 不依赖轮询父PID，避免PID复用；控制写端随唯一父进程生命周期收口。
            let _ = std::io::stdin().read(&mut byte);
            std::process::exit(0);
        })
        .map_err(|error| format!("无法建立选择器生命周期：{error}"))?;
    let initial = initial
        .ancestors()
        .find(|path| path.is_dir())
        .unwrap_or(&initial);
    let mut dialog = rfd::FileDialog::new()
        .set_title(title)
        .set_directory(initial);
    #[cfg(target_os = "linux")]
    if let Some(parent) = x11_parent() {
        dialog = dialog.set_parent(&parent);
    }
    let selected = if kind == "plugin" {
        let extension = if cfg!(target_os = "macos") {
            "dylib"
        } else {
            "so"
        };
        dialog
            .add_filter("Feathrise 插件", &[extension])
            .pick_file()
    } else {
        dialog.pick_folder()
    };
    let result = selected
        .map(|path| {
            path.into_os_string()
                .into_string()
                .map_err(|_| "所选路径不是有效UTF-8，无法传递给工程表单。".to_string())
        })
        .transpose()?;
    let bytes = serde_json::to_vec(&result).map_err(|error| error.to_string())?;
    if bytes.len() > 16 * 1024 {
        return Err("选择的路径超过长度预算。".into());
    }
    std::io::stdout()
        .write_all(&bytes)
        .map_err(|error| error.to_string())?;
    Ok(true)
}

#[cfg(target_os = "linux")]
struct X11Parent(u64);

#[cfg(target_os = "linux")]
fn x11_parent() -> Option<X11Parent> {
    let parent = std::env::var("FEATHRISE_PICKER_PARENT_WINDOW").ok()?;
    let window = parent.strip_prefix("x11:")?.parse().ok()?;
    (window != 0).then_some(X11Parent(window))
}

#[cfg(target_os = "linux")]
impl raw_window_handle::HasWindowHandle for X11Parent {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        let raw = raw_window_handle::RawWindowHandle::Xlib(
            raw_window_handle::XlibWindowHandle::new(self.0),
        );
        // 这里只向portal传父窗口ID，无跨进程指针解引用，父宿主在选择器存活期间保持窗口。
        Ok(unsafe { raw_window_handle::WindowHandle::borrow_raw(raw) })
    }
}

#[cfg(target_os = "linux")]
impl raw_window_handle::HasDisplayHandle for X11Parent {
    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        let raw = raw_window_handle::RawDisplayHandle::Xlib(
            raw_window_handle::XlibDisplayHandle::new(None, 0),
        );
        Ok(unsafe { raw_window_handle::DisplayHandle::borrow_raw(raw) })
    }
}
