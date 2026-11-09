//! 离屏作者的状态与ECS承载；所有像素工作只通过有界worker结果进入主世界。

use super::{session::AuthorSession, types::*};
use bevy::prelude::*;
use std::time::Instant;

/// Unix的ready来自当前会话实际GPU完整帧，不使用伪HWND或未渲染的成功标志。
#[derive(Resource, Debug, Default)]
pub(crate) struct EmbeddedAuthorStatus {
    pub hwnd: Option<usize>,
    pub attached: bool,
    pub ready: bool,
    pub activated: bool,
    pub error: Option<String>,
}

#[derive(Component)]
pub(super) struct OriginalWindowTarget(pub bevy::camera::RenderTarget);

pub(super) struct AuthorSurface {
    pub session: AuthorSession,
    pub geometry: Geometry,
    pub target: Handle<Image>,
    pub stable_frames: u32,
    pub sequence: u64,
    pub readback: Option<(u64, Instant)>,
    pub last_capture: Instant,
    pub last_feedback: Vec<u8>,
    pub desired_cursor: bevy::window::CursorOptions,
    pub desired_ime: (bool, Vec2),
    pub focused: bool,
    // 仅 Unix 正式作者通过 getppid 校验父进程；Windows 离屏夹具不安装该生命周期检查。
    #[cfg(unix)]
    pub parent_pid: u32,
    pub readback_completed: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

#[derive(Resource, Default)]
pub(super) struct HostSurface {
    pub token: Option<String>,
    pub image: Handle<Image>,
    pub entity: Option<Entity>,
    pub geometry: Option<Geometry>,
    pub events: Vec<bevy::window::WindowEvent>,
    pub input_overflow: bool,
}
