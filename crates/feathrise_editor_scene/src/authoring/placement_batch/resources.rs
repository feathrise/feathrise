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
    /// 验收仅读取单飞槽与固定大小游标事实，不能代替任务轮询或发布任何作者结果。
    #[cfg(feature = "validation")]
    pub(crate) fn validation_snapshot(&self) -> String {
        format!("ticket={} capture={} job={:?} paste={:?} garbage={} cleaning={}", self.ticket,
            self.capture.is_some(), self.job.as_ref().map(|job| (job.ticket, job.task.is_finished(), job.selection.as_ref().map(|(revision,_)| *revision))),
            self.paste.as_ref().map(|paste| (paste.source.records.len(),paste.records.len(),paste.cursor,paste.point.is_some(),paste.validating)),
            self.garbage.is_some(),self.cleaning.len())
    }

    #[cfg(feature = "validation")]
    pub(crate) fn validation_waiting_for_ghost_records(&self) -> bool {
        self.paste.as_ref().is_some_and(|paste| paste.records.is_empty())
    }

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
