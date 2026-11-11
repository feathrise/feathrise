//! 本轮显式常驻视口验收的阶段与观测，普通运行不安装。

use bevy::prelude::*;
use feathrise_editor_scene::standalone::embedded_validation::AuthorProbe;
use serde_json::Value;
use std::{path::PathBuf, time::Instant};

/// 五个预览显示状态共用唯一后台槽；作者与会话身份冻结后，按钮只能改变本次显示租约。
#[derive(Resource)]
pub(super) struct PreviewToolbarDriver {
    pub author: Value,
    pub session: (u32, u64, u64),
    pub camera: Option<u64>,
    pub step: usize,
    pub started: Instant,
    pub minimum_frame: u64,
    pub retry: Instant,
    pub task: Option<bevy::tasks::Task<Result<Option<Value>, String>>>,
    pub proofs: Vec<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Phase {
    Open,
    Local,
    Embedded,
    EmptyPlaying,
    EmptyPaused,
    Added,
    Undo,
    Redo,
    Tools,
    LivePreview,
    Playing,
    Paused,
    PauseStable,
    Crashed,
    Recovered,
    ClosePlaying,
    RecoveredPause,
    ReturnLocal,
    Capture,
    Finished,
    Failed,
}

/// 原生按钮按下后至少跨过一个输入采样窗口再松开；避免同帧下/上沿被折叠。
pub(super) struct GameMouseRelease {
    pub hwnd: u64,
    pub pid: u32,
    pub point: [i32; 2],
    pub pressed: Instant,
    /// 原生顶层预览走真实移动／按下／松开；旧作者子窗口继续原消息语义。
    pub native: Option<super::window_evidence::NativeMouseClick>,
}

#[derive(Resource)]
pub(super) struct Plan {
    pub project_root: PathBuf,
    pub directory: PathBuf,
    pub project_id: String,
    pub phase: Phase,
    pub started: Instant,
    pub changed: Instant,
    pub wait_logged: bool,
    pub empty_fixture: bool,
    pub live_fixture: bool,
    pub live_evidence: Option<Value>,
    pub tool_evidence: Option<Value>,
    pub preview_toolbar_evidence: Option<Value>,
    pub frames: u64,
    pub initial: Option<AuthorProbe>,
    pub added: Option<AuthorProbe>,
    /// 每次进入预览前从实际作者UI、相机、选择和历史读取；返回不能拿Playing标志代替恢复证明。
    pub presentation_baseline: Option<Value>,
    pub close_evidence: Option<Value>,
    pub pid: u32,
    pub world_id: u64,
    pub frame_before: u64,
    pub ticks_before: u64,
    /// 点击返回前的最后心跳；与返回编辑后的暂停基线分开，截图可能先于首条编辑心跳。
    pub pause_requested_after_frame: u64,
    pub current_revision: u64,
    pub pulse: Option<Entity>,
    pub game_mouse_release: Option<GameMouseRelease>,
    pub steps: Vec<Value>,
    pub error: Option<String>,
    pub completion: Option<async_channel::Receiver<Result<(), String>>>,
    pub evidence: Option<(
        (u32, u64, bool, u64, u64),
        async_channel::Receiver<Result<Option<Value>, String>>,
    )>,
    pub evidence_ready: Option<(u32, u64, bool, u64, u64)>,
    /// 宿主在游戏仍存活时独立查询的原生区域；随对应阶段写入报告，不能只相信guest的verified。
    pub region_evidence: Option<Value>,
    pub evidence_retry: Instant,
}

impl Plan {
    pub fn advance(&mut self, phase: Phase) {
        self.phase = phase;
        self.changed = Instant::now();
        self.wait_logged = false;
    }
}
