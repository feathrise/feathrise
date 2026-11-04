//! 作者呈现协议与尺寸身份；像素不进入工程低频控制通道。

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub(super) const MAX_WIDTH: u32 = 4096;
pub(super) const MAX_HEIGHT: u32 = 2304;
pub(super) const MAX_EVENTS: usize = 256;
pub(super) const MAX_PIXELS: usize = MAX_WIDTH as usize * MAX_HEIGHT as usize * 4;

/// 父PID、私有会话目录与原控制token共同确定一次工程进程；不接受只有路径的弱身份。
#[derive(Clone, Debug)]
pub(crate) struct EmbedTarget {
    pub directory: PathBuf,
    pub pid: u32,
    pub token: String,
}

impl EmbedTarget {
    pub(crate) fn from_env() -> Result<Option<Self>, String> {
        let Some(directory) = std::env::var_os("FEATHRISE_WORKSPACE_SESSION") else {
            return Ok(None);
        };
        let directory = PathBuf::from(directory);
        if !directory.is_absolute() {
            return Err("作者共享会话目录必须是绝对路径".into());
        }
        let pid = std::env::var("FEATHRISE_WORKSPACE_PARENT_PID")
            .ok()
            .and_then(|value| value.parse().ok())
            .filter(|value| *value != 0)
            .ok_or("作者共享会话缺少有效父进程身份")?;
        let token =
            std::env::var("FEATHRISE_WORKSPACE_TOKEN").map_err(|_| "作者共享会话缺少token")?;
        if token.is_empty() || token.len() > 256 {
            return Err("作者共享会话token无效".into());
        }
        Ok(Some(Self {
            directory,
            pid,
            token,
        }))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct Geometry {
    pub epoch: u64,
    pub width: u32,
    pub height: u32,
    pub scale: f32,
}

impl Geometry {
    pub fn initial() -> Self {
        Self {
            epoch: 1,
            width: 1270,
            height: 800,
            scale: 1.,
        }
    }

    pub fn checked(self) -> Result<Self, String> {
        if self.epoch == 0
            || self.width == 0
            || self.height == 0
            || self.width > MAX_WIDTH
            || self.height > MAX_HEIGHT
            || !self.scale.is_finite()
            || !(0.25..=16.).contains(&self.scale)
        {
            return Err("作者帧尺寸或比例身份无效".into());
        }
        Ok(self)
    }

    pub fn pixels(self) -> usize {
        self.width as usize * self.height as usize * 4
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct InputBatch {
    pub geometry: Geometry,
    pub active: bool,
    pub focused: bool,
    pub cursor_position: Option<Vec2>,
    pub events: Vec<bevy::window::WindowEvent>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct Feedback {
    pub epoch: u64,
    pub ime_enabled: bool,
    pub ime_position: Vec2,
    pub cursor: bevy::window::CursorOptions,
    pub cursor_icon: Option<bevy::window::SystemCursorIcon>,
}

pub(super) struct Frame {
    pub geometry: Geometry,
    pub sequence: u64,
    pub pixels: Vec<u8>,
}

pub(super) struct CapturedFrame {
    pub geometry: Geometry,
    pub sequence: u64,
    pub padded: Vec<u8>,
}

/// map回调只移交GPU句柄；整图读取、去padding与发布均由唯一帧worker执行。
pub(super) struct MappedFrame {
    pub geometry: Geometry,
    pub sequence: u64,
    pub buffer: bevy::render::render_resource::Buffer,
    pub completed: std::sync::Arc<std::sync::atomic::AtomicU64>,
}
