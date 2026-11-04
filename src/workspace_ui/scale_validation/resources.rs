//! 显式首页缩放验收的输入阶段、GPU回执和有界后台读写槽。
use super::types::*;
use bevy::{ecs::message::MessageCursor, prelude::*};
use serde_json::Value;
use std::{path::PathBuf, time::Instant};

pub(super) type Reply<T> = async_channel::Receiver<Result<T, String>>;

#[derive(Resource)]
pub(super) struct Plan {
    pub directory: PathBuf, pub preferences: PathBuf, pub author: PathBuf, pub mode: Mode,
    pub started: Instant, pub stage_started: Instant, pub stage: usize, pub frames: u32,
    pub target: Option<Entity>, pub mouse_phase: u8, pub click_seen: bool, pub issued: bool,
    pub layout_frames: u32, pub last_view: Option<Value>, pub view: Option<Value>,
    pub read: Option<Reply<Option<FileStamp>>>, pub next_read: Instant, pub disk_ready: bool,
    pub last_stamp: Option<FileStamp>, pub captured: bool, pub capture: Option<Reply<Value>>,
    pub capture_attempts: u8, pub capture_started: Option<Instant>, pub next_capture: Instant,
    pub wheel_cooldown: u8, pub wheel_events: u8,
    pub failure_started: Option<Instant>, pub failure_capture: Option<Reply<Value>>, pub failure_diagnostics: Option<Value>,
    pub proofs: Vec<Value>, pub captures: Vec<Value>, pub failure: Option<String>,
    pub report: Option<Reply<()>>, pub report_written: bool, pub exit_phase: u8,
    pub commands: MessageCursor<super::super::WorkspaceCommand>,
}
