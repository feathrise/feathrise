//! 原生预览专项的有限阶段与单飞IO槽；普通构建和普通作者工程不安装。
use bevy::prelude::*;
use serde_json::Value;
use std::{path::PathBuf, time::Instant};

pub(super) type Reply<T> = async_channel::Receiver<Result<T, String>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Phase {
    Open,
    Local,
    Added,
    Undo,
    Redo,
    Embedded,
    EditingReady,
    Launch,
    PlayingInput,
    ReturnedX,
    EditingAfterX,
    LaunchReturn,
    PlayingReturn,
    ReturnedHud,
    EditorClose,
    EditorPrompt,
    Cancelled,
    LaunchAfterCancel,
    PlayingAfterCancel,
    ReturnedAfterCancel,
    LaunchAfterAltF4,
    PlayingAfterAltF4,
    MinimizedEditorPrompt,
    MinimizedCancelled,
    FinalClose,
    FinalPrompt,
    Finished,
}

#[derive(Resource)]
pub(super) struct Plan {
    pub directory: PathBuf,
    pub project: PathBuf,
    pub token: String,
    pub root: usize,
    pub root_pid: u32,
    pub pid: u32,
    pub world_id: u64,
    pub phase: Phase,
    pub started: Instant,
    pub changed: Instant,
    pub frames: u32,
    pub initial: Option<Value>,
    pub baseline: Option<Value>,
    pub editor_baseline: Option<Value>,
    pub editor_image: Option<Reply<Value>>,
    pub editor_capture: Option<Value>,
    pub added: Option<Value>,
    pub pulse: Option<Entity>,
    pub click: Option<Entity>,
    pub click_step: u8,
    pub frame_before: u64,
    pub ticks_before: u64,
    pub paused_since: Option<Instant>,
    pub gate_name: Option<String>,
    pub gate_write: Option<Reply<()>>,
    pub gate_written: bool,
    pub gate_read: Option<Reply<Option<Value>>>,
    pub gate_retry: Instant,
    pub steps: Vec<Value>,
    pub report_write: Option<Reply<()>>,
    pub report_written: bool,
    pub failure: Option<String>,
    /// 每阶段10秒一次、最多三份；旧阶段写入未结束前继续占用同一个后台槽。
    pub diagnostic_write: Option<Reply<()>>,
    pub diagnostic_count: u8,
    pub diagnostic_next: Instant,
    pub diagnostic_error: Option<String>,
}
