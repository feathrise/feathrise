//! 显式同窗口有限验收的阶段与唯一后台槽；正常会话不创建这些状态。

use serde::Serialize;
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    thread::{self, JoinHandle},
    time::Instant,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Stage {
    Preparing,
    Start,
    CreateA,
    BadOpen,
    CreateB,
    ReopenA,
    Close,
    SaveDefault,
    DefaultUi,
    FailedDrain,
    Writing,
    Quit,
    Stopped,
}

#[derive(Clone)]
pub(super) struct Paths {
    pub parent: PathBuf,
    pub a: PathBuf,
    pub b: PathBuf,
    pub missing: PathBuf,
    pub default: PathBuf,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(super) struct Identity {
    pub pid: u32,
    pub project_id: String,
    pub project_root: PathBuf,
    pub hwnd: Option<usize>,
}

pub(super) struct NativeReport {
    pub index: usize,
    pub source: Arc<Value>,
    pub proof: Value,
}

pub(super) enum IoResult {
    Prepared(Paths),
    Native(NativeReport),
    Preferences(Value),
    Written(PathBuf),
}

pub(super) struct IoJob {
    worker: Option<JoinHandle<()>>,
    receiver: Receiver<Result<IoResult, String>>,
    cancelled: Arc<AtomicBool>,
}

impl IoJob {
    pub fn spawn(
        function: impl FnOnce(Arc<AtomicBool>) -> Result<IoResult, String> + Send + 'static,
    ) -> Result<Self, String> {
        let (sender, receiver) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let token = Arc::clone(&cancelled);
        let worker = thread::Builder::new()
            .name("Feathrise同窗口验收IO".into())
            .spawn(move || {
                let _ = sender.send(function(token));
            })
            .map_err(|error| format!("无法启动同窗口验收后台任务：{error}"))?;
        Ok(Self {
            worker: Some(worker),
            receiver,
            cancelled,
        })
    }

    pub fn poll(&mut self) -> Option<Result<IoResult, String>> {
        if !self.worker.as_ref().is_some_and(JoinHandle::is_finished) {
            return None;
        }
        // 已确认线程结束，join不承担等待IO；UI只在此处非阻塞采用完整结果。
        if self.worker.take().unwrap().join().is_err() {
            return Some(Err("同窗口验收后台线程异常退出".into()));
        }
        Some(
            self.receiver
                .try_recv()
                .unwrap_or_else(|_| Err("同窗口验收后台没有返回结果".into())),
        )
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
}

impl Drop for IoJob {
    fn drop(&mut self) {
        self.cancel(); // Drop不join；文件轮询下一次观察取消即结束。
    }
}

pub(super) struct Plan {
    pub directory: PathBuf,
    pub started: Instant,
    pub stage_started: Instant,
    pub stage: Stage,
    pub frames: u64,
    pub issued: u64,
    pub parent: Option<usize>,
    pub window_unchanged: bool,
    pub native_title: String,
    pub title_wait_started: Option<Instant>,
    pub paths: Option<Paths>,
    pub job: Option<IoJob>,
    pub initial_default: PathBuf,
    pub preferences_file: PathBuf,
    pub home: PathBuf,
    pub a: Option<Identity>,
    pub b: Option<Identity>,
    pub candidates: Vec<Identity>,
    pub reports: Vec<NativeReport>,
    pub proofs: Vec<Value>,
    pub failed: Option<String>,
    pub cancellation_sent: bool,
    pub default_disk: Option<Value>,
    pub completion: Completion,
}

#[derive(Clone)]
pub(crate) struct Completion {
    pub(super) shared: Arc<Mutex<(String, Option<Result<(), String>>)>>,
}

impl Completion {
    pub(super) fn new() -> Self {
        Self {
            shared: Arc::new(Mutex::new(("Preparing".into(), None))),
        }
    }

    /// App::run归还后调用，不在帧内阻塞；报告失败或流程中断不能被正常AppExit掩盖。
    pub(crate) fn finish(&self) -> Result<(), String> {
        let state = self.shared.lock().map_err(|_| "同窗口验收完成状态损坏")?;
        state
            .1
            .clone()
            .unwrap_or_else(|| Err(format!("同窗口验收未完整结束，最后阶段：{}", state.0)))
    }
}

impl Plan {
    pub fn stage(&mut self, stage: Stage) {
        self.stage = stage;
        self.stage_started = Instant::now();
        self.issued = self.frames;
        if let Ok(mut completion) = self.completion.shared.try_lock() {
            completion.0 = format!("{stage:?}");
        }
        eprintln!("Feathrise同窗口验收阶段：{stage:?}");
    }

    pub fn poll(&mut self) -> Option<Result<IoResult, String>> {
        let result = self.job.as_mut()?.poll()?;
        self.job = None;
        Some(result)
    }
}

/// 主窗口句柄来自Win32，工程清单通常带verbatim前缀；只消除表示差异，不访问磁盘。
pub(super) fn same_path(a: &Path, b: &Path) -> bool {
    a == b || normalized(a) == normalized(b)
}

pub(super) fn inside(path: &Path, parent: &Path) -> bool {
    normalized(path).starts_with(&(normalized(parent) + "/"))
}

fn normalized(path: &Path) -> String {
    let path = path.to_string_lossy();
    let path = if let Some(rest) = path.strip_prefix("\\\\?\\UNC\\") {
        format!("\\\\{rest}")
    } else {
        path.strip_prefix("\\\\?\\").unwrap_or(&path).to_owned()
    };
    let path = path.replace('\\', "/").trim_end_matches('/').to_owned();
    if cfg!(windows) {
        path.to_ascii_lowercase()
    } else {
        path
    }
}
