//! 视口选择与窗口布局的有界快照；进程和作者文档的所有权留给各自功能域。

use super::ViewportMode;
use bevy::prelude::*;

#[derive(Resource, Debug)]
pub(crate) struct ViewportModeState {
    pub selected: ViewportMode,
    pub revision: u64,
    pub status: String,
}

impl Default for ViewportModeState {
    fn default() -> Self {
        Self {
            selected: ViewportMode::default(),
            revision: 1,
            status: String::new(),
        }
    }
}

/// 游戏生命周期在构建、工程运行、退出及未确认清理期间持有此门；不能借切换跳过进程收口。
#[derive(Resource, Debug, Default)]
pub(crate) struct ViewportModeAvailability {
    pub blocked_reason: Option<String>,
}

/// 只保留本次停止的返回意图；后台真实收口并通过原切换门后才改变选择及个人偏好。
#[derive(Resource, Default)]
pub(super) struct CompilationReturn {
    pub(super) selection_revision: Option<u64>,
}

/// 下拉打开与关闭当帧都占有输入，后续释放继续沿用作者层的鼠标静默门。
#[derive(Resource, Default)]
pub(crate) struct BuildProfileMenuState {
    pub(super) open: bool,
    pub(super) captured: bool,
}

impl BuildProfileMenuState {
    pub(crate) fn captures_input(&self) -> bool { self.captured }
}

/// 以作者客户区为原点的物理像素矩形；隐藏时保留末次有效尺寸，避免最小化触发零尺寸重建。
/// HWND 与 PID 必须一起验证；focused 只表示本主窗口在前台，不授权子进程抢占系统焦点。
#[derive(Resource, Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ViewportGeometry {
    pub parent_hwnd: usize,
    pub parent_pid: u32,
    pub rect: [i32; 4],
    /// child局部物理坐标的作者控件留孔；最多32项，不能通过缩小视口改变相机投影。
    pub excluded_rects: Vec<[u32; 4]>,
    /// XYZ 的实际绘制轮廓；矩形外框及负轴圆环内部保持工程画面透明。
    pub excluded_shapes: Vec<feathrise_editor_protocol::ViewportUiShape>,
    pub visible: bool,
    pub focused: bool,
    pub revision: u64,
}
