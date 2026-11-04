//! 每个自有App只持有一个输入阶段和一笔异步读/写，桌面观察器的ack不代替业务成功。
use super::types::*;
use bevy::prelude::*;
use serde_json::Value;
use std::{path::PathBuf,time::Instant};
pub(super) type Reply<T> = async_channel::Receiver<Result<T,String>>;
#[derive(Resource)]
pub(super) struct Plan {
    pub directory:PathBuf,pub project:PathBuf,pub token:String,pub role:Role,pub stage:u8,pub started:Instant,pub stage_started:Instant,
    pub mouse_phase:u8,pub target:Option<Entity>,pub clicked:bool,pub stable:u32,pub saw_project:bool,
    pub image:Option<Reply<Value>>,pub image_done:bool,pub attempts:u8,pub next_image:Instant,
    pub writing:Option<Reply<()>>,pub ready_written:bool,pub reading:Option<Reply<Option<FileStamp>>>,pub next_read:Instant,
    pub arm_written:bool,pub input_armed:bool,
    pub input_rect:Option<[f32;4]>,
    pub proofs:Vec<Value>,pub captures:Vec<Value>,pub failure:Option<String>,pub final_write:Option<Reply<()>>,pub final_done:bool,pub exit_issued:bool,
}
