//! 稳定实例选择、单飞引用校验与数值拖动事务；来源ID和实例ID分别保存。

use bevy::{prelude::*, tasks::Task};
use feathrise_scene_content::{SceneId, SceneInstanceId, SceneInstancePlacement};
use std::sync::Arc;
pub(super) struct Pending {
    pub scene_id: SceneId,
    pub revision: u64,
    pub selected: SceneInstanceId,
    pub references: Arc<[SceneInstancePlacement]>,
    pub task: Task<Result<(), String>>,
}
pub(super) struct Scrub {
    pub entity: Entity,
    pub selected: SceneInstanceId,
    pub scene_id: SceneId,
    pub before: Arc<[SceneInstancePlacement]>,
    pub expected_revision: u64,
    pub dirty_before: bool,
}
pub(super) struct RenamePending {
    pub scene_id: SceneId,
    pub revision: u64,
    pub task: Task<Result<Arc<[SceneInstancePlacement]>, String>>,
}
#[derive(Resource, Default)]
pub(crate) struct SubsceneEditorState {
    pub scene_id: Option<SceneId>,
    pub selected: Option<SceneInstanceId>,
    /// 当前滚动窗口的首行，固定行池不限制完整引用数量。
    pub list_start: usize,
    pub(super) pending: Option<Pending>,
    pub(super) scrub: Option<Scrub>,
    pub(super) rename: Option<RenamePending>,
    /// 名称结果取消或旧redo退役时仍只占一个CPU槽，完整引用/历史不在帧内集中析构。
    pub(super) retirement: Option<Task<()>>,
}

impl SubsceneEditorState { pub(crate) fn busy(&self) -> bool { self.pending.is_some() || self.scrub.is_some() || self.rename.is_some() || self.retirement.is_some() } }

impl SubsceneEditorState {
    pub(crate) fn embedded_target(&self, session: &crate::authoring::EditorAuthoringSession) -> Option<SceneInstanceId> {
        self.scrub.as_ref().filter(|scrub| scrub.expected_revision == session.revision &&
            session.draft.as_ref().is_some_and(|draft| draft.scene_id == scrub.scene_id))
            .map(|scrub| scrub.selected.clone())
    }
}
