//! 一个底栏会话保留最近回执及一条待处理诊断，不保存配置或维护操作日志。

use std::sync::Arc;

use bevy::prelude::*;
use feathrise_scene_content::SceneId;

use super::types::StatusPhase;
use crate::types::{EditorStatusKind, EditorStatusOperation, EditorStatusResult};

#[derive(Resource, Default)]
pub(crate) struct EditorStatusBarState {
    pub(super) scene_id: Option<SceneId>,
    pub(super) seen_results: [u64; EditorStatusOperation::COUNT],
    pub(super) retained: Option<(EditorStatusKind, Arc<str>)>,
    pub(super) notices: [Option<EditorStatusResult>; EditorStatusOperation::COUNT],
    pub(super) retained_runtime: Option<(EditorStatusKind, Arc<str>)>,
    pub(super) latest_result: Option<EditorStatusResult>,
    pub(super) runtime_errors: [Option<Arc<str>>; 5],
    pub(super) runtime_details: [Option<(super::types::RuntimePhase, Arc<str>)>; 5],
    pub(super) phase: StatusPhase,
    pub(super) summary: String,
    pub(super) task: String,
    pub(super) context: String,
    pub(super) detail: String,
    pub(super) scroll_revision: u64,
    pub(super) detail_dirty: bool,
    pub(super) observed_dirty: bool,
    pub(super) observed_status: String,
    pub(super) popup_open: bool,
    pub(super) popup_hovered: bool,
    pub(super) captured_this_frame: bool,
    pub(super) copy_result: String,
    pub(super) copy_failed: bool,
    pub(super) panel_status: std::collections::HashMap<Entity, (&'static str, Arc<str>)>, // 固定UI反馈源，不缓存场景/资产全量数据。
}

impl EditorStatusBarState {
    pub(crate) fn overlays_viewport(&self) -> bool { self.popup_open }
    pub(crate) fn owns_mouse(&self) -> bool {
        (self.popup_open && self.popup_hovered) || self.captured_this_frame
    }
}
