//! 沿线确认先分帧冻结原有摆放，再由原地基任务准备最终数组、历史和显示队列。
use super::plan::Plan;
use super::super::{CourtyardAuthoringState, PendingVisualSync, operations::run_ground_operation,
    state::{OperationIdentity, OperationResult}, types::CourtyardGroundWork};
use crate::authoring::{CourtyardBatchChange, EditorAuthoringSession, EditorCommand, EditorSceneDraft,
    buildings::CourtyardBuildingValidator, placement_append::{CAPTURE_BUDGET, PlacementSource, capture_prefix, validated_append},
    placement_deletion::PlacementDeletionIdentity};
use crate::resources::SceneEditorState;
use feathrise_scene_content::ScenePlacementRecord;
use std::sync::Arc;

#[cfg(test)]
mod tests;

pub(in crate::authoring::courtyards) struct GroundStart {
    pub(super) plan: Arc<Plan>,
    pub(super) id_prefix: String,
    pub(super) validator: CourtyardBuildingValidator,
}

impl GroundStart {
    pub(in crate::authoring::courtyards) fn run(self, mut result: OperationResult) -> OperationResult {
        match self.plan.candidates(&self.id_prefix) {
            Ok(candidates) => run_ground_operation(result, CourtyardGroundWork {
                validator: self.validator, candidates, excluded_ids: Default::default(), query: None,
            }),
            Err(error) => { result.result = Err(error); result }
        }
    }
}

pub(in crate::authoring::courtyards) struct WallSubmission {
    pub(in crate::authoring::courtyards) epoch: u64,
    source: PlacementSource,
    document: PlacementDeletionIdentity,
    pose_revision: u64,
    _route: Arc<Plan>, // 取消或成功清空工具路线时，最后一份复杂折线也留给本任务后台退役。
    prefix: Vec<ScenePlacementRecord>,
    pub(in crate::authoring::courtyards) start: Option<GroundStart>,
    pub(in crate::authoring::courtyards) ready: Option<WallAddition>,
}

impl WallSubmission {
    pub(super) fn new(epoch: u64, session: &EditorAuthoringSession, draft: &EditorSceneDraft, start: GroundStart) -> Self {
        Self { epoch, source: PlacementSource::of(draft), document: session.placement_deletions.identity(),
            pose_revision: session.placement_derived_pose_revision, _route: start.plan.clone(),
            prefix: Vec::new(), start: Some(start), ready: None }
    }

    pub(in crate::authoring::courtyards) fn matches(&self, session: &EditorAuthoringSession) -> bool {
        self.pose_revision == session.placement_derived_pose_revision && self.document.matches(&session.placement_deletions)
            && session.draft.as_ref().is_some_and(|draft| self.source == PlacementSource::of(draft))
    }

    pub(in crate::authoring::courtyards) fn capture(&mut self, draft: &EditorSceneDraft) -> Result<bool, String> {
        let end = self.prefix.len().saturating_add(CAPTURE_BUDGET).min(self.source.len);
        capture_prefix(&mut self.prefix, &draft.placements.placements, self.source.len, end)?;
        Ok(end == self.source.len)
    }

    /// 精确地基的最后一次 worker 同时完成前缀查重、历史计费与全部深拷贝。
    pub(in crate::authoring::courtyards) fn prepare(&mut self, added: Vec<ScenePlacementRecord>, id: &OperationIdentity) -> Result<(), String> {
        let mut records = std::mem::take(&mut self.prefix);
        if records.len() != self.source.len { return Err("围墙的原摆放快照尚未完整".into()); }
        let bounds = self.source.bounds.ok_or("铺设围墙时场景世界范围无效")?;
        let water_count = validated_append(&records, &added, self.source.water_count, false, &id.scene_id, bounds, None)?;
        let history = Arc::new(CourtyardBatchChange::for_wall(added, self.source.len)?);
        records.try_reserve_exact(history.added.len()).map_err(|_| "无法分配围墙摆放记录")?;
        records.extend_from_slice(&history.added);
        let updates = history.added.iter().cloned().enumerate().map(|(i, record)| (self.source.len + i, record, true));
        let visuals = PendingVisualSync::prepare(id.scene_id.clone(), id.structure_revision.wrapping_add(1), updates);
        self.ready = Some(WallAddition { records, water_count, history, visuals });
        Ok(())
    }
}

pub(in crate::authoring::courtyards) struct WallAddition {
    records: Vec<ScenePlacementRecord>,
    water_count: usize,
    history: Arc<CourtyardBatchChange>,
    visuals: PendingVisualSync,
}

impl WallAddition {
    /// 调用方已在本帧核对完整身份。返回旧数组，与整条路线一起交还原任务槽退役。
    pub(in crate::authoring::courtyards) fn apply(self, session: &mut EditorAuthoringSession, editor: &mut SceneEditorState,
        state: &mut CourtyardAuthoringState) -> Vec<ScenePlacementRecord> {
        let draft = session.draft.as_mut().expect("已核对当前场景");
        let previous = std::mem::replace(&mut draft.placements.placements, self.records);
        draft.placement_water_surface_count = self.water_count;
        session.push_bounded_water_history(EditorCommand::CourtyardBatch(self.history));
        session.mark_placement_structure_changed(editor);
        state.adopt_visuals(self.visuals);
        previous
    }
}
