//! 预览三键专项的固定阶段、单飞证据与原鼠标输入脉冲，普通构建不安装。
use bevy::prelude::*;
use serde_json::Value;
use std::{path::PathBuf,time::Instant};

pub(super) type Reply<T> = async_channel::Receiver<Result<T,String>>;

#[derive(Resource)]
pub(super) struct Plan {
    pub directory:PathBuf,
    pub project:PathBuf,
    pub started:Instant,
    pub changed:Instant,
    pub stage:u8,
    pub frames:u32,
    pub map_requested:bool,
    pub mouse:u8,
    pub target:Option<Entity>,
    pub clicked:bool,
    pub root:usize,
    pub root_pid:u32,
    pub author_hwnd:usize,
    pub pid:u32,
    pub world_id:u64,
    pub baseline:Option<Value>,
    pub baseline_chrome:Option<Value>,
    pub min_frame:u64,
    pub game_task:Option<Reply<Option<Value>>>,
    pub game:Option<Value>,
    pub retry:Instant,
    pub image:Option<Reply<Value>>,
    pub image_ready:bool,
    pub checkpoint_done:bool,
    pub attempts:u8,
    pub minimize:Option<Reply<Value>>,
    pub minimize_proof:Option<Value>,
    pub proofs:Vec<Value>,
    pub captures:Vec<Value>,
    pub write:Option<Reply<()>>,
    pub report_written:bool,
    pub failure:Option<String>,
    pub native_token:Option<String>,
    pub gate_written:bool,
    pub gate_write:Option<Reply<()>>,
    pub gate_read:Option<Reply<Option<super::types::FileStamp>>>,
    pub gate_ack:bool,
    pub gate_retry:Instant,
    pub diagnostic:Option<Reply<()>>,
    pub diagnostic_count:u8,
    pub diagnostic_next:Instant,
    pub game_last_result:Option<Value>,
    pub diagnostic_error_seen:bool,
    pub failure_status:Option<Value>,
    pub failure_log:Option<Value>,
    /// 安装时显式选择诊断；默认专项不引入诊断IO等待，避免日志时序影响真实输入复验。
    pub native_sync_trace:bool,
    /// 两个原生输入门共用32份上限；只比较固定DTO语义，不因每帧时间推进而落盘。
    pub native_sync_write:Option<Reply<()>>,
    pub native_sync_count:u8,
    pub native_sync_last:Option<crate::window_chrome::resources::NativeSyncDiagnostics>,
    pub native_sync_last_gate:Option<String>,
}
