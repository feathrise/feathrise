//! 宿主常驻会话资源：固定队列、真实回执和唯一后台线程的拥有权。

use bevy::prelude::*;
use feathrise_editor_protocol::{CameraState, Envelope, Message, Mode, SessionIdentity};
use std::time::Instant;
use std::{
    path::PathBuf,
    sync::{Arc, mpsc},
    thread::JoinHandle,
};

#[derive(Resource)]
pub(super) struct Settings {
    pub project: crate::project::Project,
    pub target: PathBuf,
}

#[derive(Resource, Debug, Default)]
pub(crate) struct EmbeddedStatus {
    pub active: bool,
    pub ready: bool,
    pub pid: u32,
    pub world_id: u64,
    /// 仅实际函数补丁采用回执推进；不与场景修订、心跳帧数或源码代次混用。
    pub patch_epoch: u64,
    pub playing: bool,
    pub applied_revision: u64,
    pub frames: u64,
    pub ticks: u64,
    /// 与frames/ticks同一条心跳的观测模式；不代替可靠ModeApplied的操作授权。
    pub observed_mode: Option<Mode>,
    pub starts: u64,
    pub completed: u64,
    pub error: Option<String>,
}

/// 顶栏只读呈现；握手成功与当前场景已经采用是两个阶段，不改变原进程生命周期。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ConnectionState {
    Disconnected,
    Starting,
    Synchronizing,
    Connected,
    Stopping,
    Restarting,
    Failed,
}

pub(super) struct Outgoing {
    pub generation: u64,
    pub message: Message,
    pub payload: Option<Arc<Vec<u8>>>,
}
impl Outgoing {
    pub fn control(message: Message) -> Self {
        Self {
            generation: 0,
            message,
            payload: None,
        }
    }
}
pub(super) enum Event {
    Progress(String),
    HotpatchBaseline(Arc<crate::code_iteration::hotpatch::FatBaseline>),
    Protocol(Envelope),
}

pub(super) struct Active {
    pub control: crate::build_process::BuildControl,
    pub sender: mpsc::SyncSender<Outgoing>,
    pub receiver: mpsc::Receiver<Event>,
    pub result: mpsc::Receiver<Result<(), String>>,
    pub thread: JoinHandle<()>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PlayWaitPhase {
    Connecting,
    Preparing,
    Applying,
    Readiness,
}

/// 只比较有界作者/事务身份；普通心跳和渲染帧不算草稿采用进展。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PlayWaitProgress {
    pub phase: PlayWaitPhase,
    pub reason: Option<&'static str>,
    pub author_revision: u64,
    pub ready_revision: u64,
    pub capture_pending: bool,
    pub sent_revision: u64,
    pub applied_revision: u64,
}

pub(super) struct PlayWait {
    pub progress: PlayWaitProgress,
    /// Ready 前没有准备计时；首次 Cargo 构建不占用作者准备预算。
    pub last_progress: Option<Instant>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CodeApplyStage {
    Returning,
    Draining,
    Starting,
    Synchronizing,
    Restoring,
}

pub(super) struct CodeApply {
    pub prepared: Arc<crate::native_play::PreparedGame>,
    pub token: u64,
    pub resume_playing: bool,
    pub stage: CodeApplyStage,
    pub started: Instant,
    pub session: Option<u64>,
    pub restored_revision: Option<u64>,
    pub mode_confirmed: bool,
    pub progress: Option<(u64, u64, u64, bool)>,
}

#[derive(Default)]
pub(super) struct HotpatchRuntime {
    pub baseline: Option<Arc<crate::code_iteration::hotpatch::FatBaseline>>,
    pub ready: Option<crate::code_iteration::hotpatch::PatchTarget>,
    pub flight: Option<HotpatchFlight>,
}

pub(super) struct HotpatchFlight {
    pub prepared: Arc<crate::code_iteration::hotpatch::PreparedPatch>,
    pub token: u64,
    pub started: Instant,
    /// Some表示已经进入可靠控制队列，之后必须等真实结果，不能因新保存而假称撤回。
    pub sent: Option<(u64, Instant)>,
    pub timeout_reported: bool,
}

#[derive(Default)]
pub(super) struct Runtime {
    pub active: Option<Active>,
    pub exit_requested: bool,
    pub cleanup_failure: Option<String>,
    pub attempted_selection: u64,
    /// 与attempted_selection一起锁定本次Job身份；构建期间不可读取后来UI选项。
    pub attempted_profile: Option<feathrise_editor_core::ProjectBuildProfile>,
    pub attempted_backend: Option<crate::native_play::ProjectBuildBackend>,
    /// 显式停止只撤销本次未就绪启动；保留已尝试的选择/配置，防止下一帧自动重编译。
    pub compilation_stopped: bool,
    /// 停止时的界面输入与原Job身份分开记录；同帧已改配置也不能在排空后偷偷重启。
    pub compilation_stop_input: Option<(u64, feathrise_editor_core::ProjectBuildProfile)>,
    pub identity: Option<SessionIdentity>,
    pub next_request: u64,
    pub waiting_mode: Option<(u64, Mode)>,
    pub waiting_revision: u64,
    pub pending_play: bool,
    pub play_wait: Option<PlayWait>,
    pub pending_edit: bool,
    pub close_return_requested: bool,
    /// 从关闭意图开始计时，不能依赖返回命令成功入队；有心跳但控制背压也必须有界收口。
    pub close_return_started: Option<Instant>,
    pub restart_requested: bool,
    /// 代码应用有独立意图，不能借错误重连固定进入Playing而改变作者原模式。
    pub code_apply: Option<CodeApply>,
    pub code_apply_status: Option<crate::native_play::CodeApplyStatus>,
    pub hotpatch: HotpatchRuntime,
    pub snapshot_in_flight: Option<u64>,
    pub sent_revision: u64,
    pub geometry_revision: u64,
    pub camera: Option<CameraState>,
    pub author_view: Option<feathrise_editor_protocol::AuthorViewState>,
    pub scene_id: Option<String>,
    pub overlay_generation: u64,
    pub tools_generation: u64,
    pub mode_started: Option<Instant>,
    pub snapshot_started: Option<Instant>,
    pub capture_error: Option<String>,
    pub previews: super::previews::PreviewFlights,
}
impl Drop for Runtime {
    fn drop(&mut self) {
        if let Some(active) = &self.active {
            active.control.cancel();
        }
    }
}
