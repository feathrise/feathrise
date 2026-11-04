//! 稳定实例选择、单飞引用校验与数值拖动事务；来源ID和实例ID分别保存。

use bevy::{prelude::*, tasks::Task};
use feathrise_map_content::{MapId, MapInstanceId, MapInstancePlacement};
use std::sync::Arc;
pub(super) struct Pending {
    pub map_id: MapId,
    pub revision: u64,
    pub selected: MapInstanceId,
    pub references: Arc<[MapInstancePlacement]>,
    pub task: Task<Result<(), String>>,
}
pub(super) struct Scrub {
    pub entity: Entity,
    pub selected: MapInstanceId,
    pub map_id: MapId,
    pub before: Arc<[MapInstancePlacement]>,
    pub expected_revision: u64,
    pub dirty_before: bool,
}
pub(super) struct RenamePending {
    pub map: MapId,
    pub revision: u64,
    pub task: Task<Result<Arc<[MapInstancePlacement]>, String>>,
}
#[derive(Resource, Default)]
pub(crate) struct SubMapEditorState {
    pub map_id: Option<MapId>,
    pub selected: Option<MapInstanceId>,
    /// 当前滚动窗口的首行，固定行池不限制完整引用数量。
    pub list_start: usize,
    pub(super) pending: Option<Pending>,
    pub(super) scrub: Option<Scrub>,
    pub(super) rename: Option<RenamePending>,
    /// 名称结果取消或旧redo退役时仍只占一个CPU槽，完整引用/历史不在帧内集中析构。
    pub(super) retirement: Option<Task<()>>,
}

impl SubMapEditorState { pub(crate) fn busy(&self) -> bool { self.pending.is_some() || self.scrub.is_some() || self.rename.is_some() || self.retirement.is_some() } }

impl SubMapEditorState {
    pub(crate) fn embedded_target(&self, session: &crate::authoring::EditorAuthoringSession) -> Option<MapInstanceId> {
        self.scrub.as_ref().filter(|scrub| scrub.expected_revision == session.revision &&
            session.draft.as_ref().is_some_and(|draft| draft.map_id == scrub.map_id))
            .map(|scrub| scrub.selected.clone())
    }
}
