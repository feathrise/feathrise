//! 首版工程运行后台槽的所有权；退出时只取消本实例自己的工具与游戏Job。

use crate::{build_process::BuildControl, project::Project};
use bevy::prelude::*;
use std::{path::PathBuf, sync::{Arc, mpsc::Receiver}, thread::JoinHandle};

/// 应用回执只描述本次明确请求；源码代次与工程身份由代码迭代协调器绑定token。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CodeApplyState {
    Applying,
    Applied,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CodeApplyStatus {
    pub token: u64,
    pub state: CodeApplyState,
    pub error: Option<String>,
}

pub(super) struct PendingCodeApply {
    pub prepared: Arc<super::PreparedGame>,
    pub request: feathrise_editor_map::standalone::RunSavedProject,
    pub token: u64,
}

/// 固定大小的启动/收口回执；计数表示后台尝试，不把构建开始冒称游戏或画面已经就绪。
#[derive(Resource, Default, Clone, Debug)]
pub(crate) struct NativePlayStatus {
    pub started: u64,
    pub completed: u64,
    pub active: bool,
    /// 已受理启动、尚未收到实际运行 EXE 的 Job 创建回执；游戏运行期间不再显示准备动画。
    pub preparing: bool,
    pub last_result: Option<Result<(), String>>,
    pub last_exit_code: Option<u32>,
}

#[derive(Resource)]
pub(super) struct NativePlaySettings {
    pub project: Project,
    pub target: PathBuf,
}

pub(super) struct ActivePlay {
    pub control: BuildControl,
    pub progress_rx: Receiver<String>,
    pub result_rx: Receiver<Result<u32, String>>,
    pub handle: JoinHandle<()>,
}

// Receiver由主线程独占，使用NonSend资源而非锁；帧内只做try_recv与is_finished。
#[derive(Default)]
pub(super) struct NativePlayRuntime {
    pub active: Option<ActivePlay>,
    pub exit_requested: bool,
    pub cleanup_failed: bool,
    /// 冻结普通工程运行的地图与配置；应用版本只重启这一请求，不回读后来改变的UI选择。
    pub active_request: Option<feathrise_editor_map::standalone::RunSavedProject>,
    /// 普通启动独享创建回执，不与代码采用的 token 回执竞争同一接收端。
    pub launch_started: Option<Receiver<()>>,
    pub code_apply_pending: Option<PendingCodeApply>,
    pub code_apply_status: Option<CodeApplyStatus>,
    pub code_apply_started: Option<Receiver<()>>,
    pub code_apply_active_token: Option<u64>,
}

impl Drop for NativePlayRuntime {
    fn drop(&mut self) {
        if let Some(active) = self.active.as_ref() {
            active.control.cancel(); // 后台run收口精确Job，最后进程退出也有KILL_ON_JOB_CLOSE兜底。
        }
    }
}
