//! 顶栏只缓存固定数量的原控件实体与菜单状态，不复制作者数据或保存历史。
use super::types::Menu;
use bevy::prelude::*;

#[derive(Resource, Default)]
pub(crate) struct ChromeState {
    pub(super) menu: Option<Menu>,
    pub(super) captured: bool,
    pub(super) compact: bool,
    pub(super) narrow: bool,
    pub(super) maximized: bool,
    pub(super) minimized: bool,
    pub(super) focused: bool,
}
impl ChromeState {
    pub(crate) fn captures_input(&self) -> bool {
        self.menu.is_some() || self.captured
    }
}

#[derive(Resource, Default)]
pub(super) struct ChromeParts(pub Option<HeaderParts>);

/// 保留旧专项的只读布局格式；原生预览不再创建替代标题栏，该值始终为空。
#[derive(Resource, Default, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PreviewChromeLayout {
    pub(crate) visible: bool,
    pub(crate) bottom: u32,
}

pub(super) struct HeaderParts {
    pub header: Entity,
    pub spacer: Entity,
    pub project: Option<Entity>,
    pub history: Vec<Entity>,
    pub selection: Entity,
    pub settings: Entity,
    pub modes: Option<Entity>,
    pub profile: Option<Entity>,
    pub run: Entity,
    pub layout_controls: Vec<Entity>,
    pub more_toggle: Entity,
    pub more: Entity,
    pub backdrop: Entity,
    pub captions: Entity,
}

/// 仅验收构建记录afterLast原生同步的固定快照；不含帧计数，不在运行帧分配或写文件。
/// 显式自有预览场景按状态变化异步落盘，普通构建完全不包含该资源。
#[cfg(all(feature = "validation", target_os = "windows"))]
#[derive(Resource, Default, Clone, Copy, Debug, PartialEq, serde::Serialize)]
pub(super) struct NativeSyncDiagnostics {
    pub phase: &'static str,
    pub own: usize,
    pub root: usize,
    pub root_pid: u32,
    pub actual_root_pid: u32,
    pub parent: usize,
    pub ancestor: usize,
    pub valid_root: bool,
    pub focused: bool,
    pub window_focused: bool,
    pub foreground: usize,
    pub capture: usize,
    pub geometry_visible: bool,
    pub drag: [i32; 4],
    pub minimize: [i32; 4],
    pub maximize: [i32; 4],
    pub close: [i32; 4],
    pub cursor_valid: bool,
    pub cursor: [i32; 2],
    pub held: bool,
    pub caption_down: Option<&'static str>,
    pub query_matches: usize,
    pub caption_matches: usize,
    pub buttons: [Option<NativeCaptionDiagnostics>; 3],
}

#[cfg(all(feature = "validation", target_os = "windows"))]
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
pub(super) struct NativeCaptionDiagnostics {
    pub action: &'static str,
    pub entity: u64,
    pub rect: [i32; 4],
    pub before_interaction: &'static str,
    pub wanted: &'static str,
    pub relative_over: bool,
    pub relative_normalized: Option<[f32; 2]>,
    pub background_before: [f32; 4],
    pub background_after: [f32; 4],
    pub colors: [[f32; 4]; 3],
}
