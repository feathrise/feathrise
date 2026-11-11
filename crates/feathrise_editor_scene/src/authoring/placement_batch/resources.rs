//! 剪贴板全进程保留；当前事务和模型清理共用有界状态，取消不假定CPU任务已终止。
use super::types::*;
use bevy::prelude::*;
use std::sync::Arc;

#[derive(Resource, Default)]
pub(crate) struct PlacementBatchState {
    pub(super) clipboard: Option<Arc<ClipboardData>>,
    pub(super) capture: Option<Capture>,
    pub(super) paste: Option<PasteSession>,
    pub(super) job: Option<BatchJob>,
    pub(super) waiting: Option<Box<PasteWork>>,
    pub(super) ticket: u64,
    pub(super) cleaning: Vec<Entity>,
    pub(super) owns_mouse: bool,
    pub(super) selection_request: Option<String>,
    pub(super) garbage: Option<CancelledBatch>,
}
impl PlacementBatchState {
    pub(crate) fn grid_reference(&self) -> Option<(Vec3, Vec3, feathrise_editor_core::EditorTranslationSnap)> {
        let paste = self.paste.as_ref()?;
        let (key, point) = paste.point?;
        Some((paste.source.origin, point, key.snap)) // 使用真正已显示的共同落点，不借用另一图的选择中心。
    }
    pub(crate) fn busy(&self) -> bool {
        self.capture.is_some()
            || self.paste.is_some()
            || self.job.is_some()
            || !self.cleaning.is_empty()
            || self.garbage.is_some()
    }
    pub(crate) fn owns_mouse(&self) -> bool {
        self.owns_mouse
    }
}
