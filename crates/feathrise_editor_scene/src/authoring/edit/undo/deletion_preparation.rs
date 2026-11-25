//! 整组历史分帧捕获后在同一 CPU 槽准备删除票据与沿线围墙的完整摆放版本。
//!
//! 【功能设计】冻结历史留在原栈，每帧只捕获128份草稿摆放或增删 ID。
//! 真正启动worker时才共享逻辑表，任务及迟到结果完全退役前不复用该槽。

use bevy::{prelude::*, tasks::{AsyncComputeTaskPool, Task, futures::check_ready}};
use feathrise_scene_content::{SceneId, ScenePlacementRecord};
use std::sync::Arc;
use super::wall::WallHistory;
use crate::authoring::placement_append::{CAPTURE_BUDGET, PlacementSource};

use crate::{authoring::{CourtyardBatchChange, EditorAuthoringSession, EditorCommand}, resources::SceneEditorState};
use crate::authoring::placement_deletion::{PlacementDeletionIdentity, PreparedPlacementDeletions};

struct HistoryIdentity {
    scene_id: SceneId,
    document: PlacementDeletionIdentity,
    deletion: u64,
    author: u64,
    structure: u64,
    derived_pose: u64,
    undoing: bool,
    top: Arc<CourtyardBatchChange>,
    undo_len: usize,
    redo_len: usize,
    wall: Option<PlacementSource>,
}

impl HistoryIdentity {
    fn new(session: &EditorAuthoringSession, undoing: bool, command: &EditorCommand) -> Option<Self> {
        let EditorCommand::CourtyardBatch(change) = command.unscoped() else { return None; };
        if change.added.is_empty() && change.removed.is_empty() { return None; }
        let draft = session.draft.as_ref()?;
        Some(Self {
            scene_id: draft.scene_id.clone(),
            document: session.placement_deletions.identity(),
            deletion: session.placement_deletions.revision(),
            author: session.revision,
            structure: session.placement_structure_revision,
            derived_pose: session.placement_derived_pose_revision,
            undoing,
            top: change.clone(),
            undo_len: session.undo.len(),
            redo_len: session.redo.len(),
            wall: change.wall_base.map(|_| PlacementSource::of(draft)),
        })
    }

    fn matches(&self, session: &EditorAuthoringSession, editor: &SceneEditorState) -> bool {
        if !editor.open || editor.close_requested || editor.scene_transition_in_progress
            || editor.pending_scene_transition.is_some() || session.saving
            || editor.selected_scene_id.as_ref() != Some(&self.scene_id)
            || session.draft.as_ref().is_none_or(|draft| draft.scene_id != self.scene_id)
            || !self.document.matches(&session.placement_deletions)
            || self.deletion != session.placement_deletions.revision()
            || self.author != session.revision || self.structure != session.placement_structure_revision
            || self.derived_pose != session.placement_derived_pose_revision
            || self.undo_len != session.undo.len() || self.redo_len != session.redo.len()
            || self.wall.as_ref().is_some_and(|source| session.draft.as_ref().is_none_or(|draft| *source != PlacementSource::of(draft)))
            || !(if self.undoing { editor.undo_requested } else { editor.redo_requested && !editor.undo_requested })
        { return false; }
        let command = if self.undoing { session.undo.last() } else { session.redo.last() };
        command.is_some_and(|command| match command.unscoped() {
            EditorCommand::CourtyardBatch(change) => Arc::ptr_eq(change, &self.top),
            _ => false,
        })
    }
}

struct HistoryCapture {
    identity: HistoryIdentity,
    cursor: usize,
    deleted: Vec<String>,
    restored: Vec<String>,
    records: Vec<ScenePlacementRecord>,
}

pub(super) struct PreparedHistory {
    pub(super) deletions: PreparedPlacementDeletions,
    pub(super) wall: Option<WallHistory>,
}

impl PreparedHistory {
    fn tickets(deletions: PreparedPlacementDeletions) -> Self { Self { deletions, wall: None } }
}

enum JobOutput { Prepared(Result<PreparedHistory, String>), Retired }
struct PreparationJob {
    identity: Option<HistoryIdentity>, // None是同槽后台析构，完成前仍施加single-flight背压。
    cancelled: bool,
    task: Task<JobOutput>,
}

#[derive(Default)]
pub(crate) struct HistoryDeletionPreparation {
    capture: Option<HistoryCapture>,
    job: Option<PreparationJob>,
    ready: Option<(HistoryIdentity, Result<PreparedHistory, String>)>,
}

pub(super) enum Preparation { Unneeded, Waiting, Ready(PreparedHistory), Failed(String) }

impl HistoryDeletionPreparation {
    fn cancel_request(identity: &HistoryIdentity, editor: &mut SceneEditorState) {
        if identity.undoing { editor.undo_requested = false; } else { editor.redo_requested = false; }
        editor.status = "整组历史来源已经变化，旧请求未应用；历史记录已保留".into();
    }

    pub(super) fn retire<T: Send + 'static>(&mut self, value: T) {
        debug_assert!(self.job.is_none());
        self.job = Some(PreparationJob {
            identity: None, cancelled: false,
            task: AsyncComputeTaskPool::get().spawn(async move { drop(value); JobOutput::Retired }),
        });
    }

    /// 即使没有新的请求，也非阻塞排空原任务；取消不能通过丢句柄假装worker已停止。
    pub(super) fn maintain(&mut self, session: &EditorAuthoringSession, editor: &mut SceneEditorState) {
        if self.capture.as_ref().is_some_and(|capture| !capture.identity.matches(session, editor)) {
            Self::cancel_request(&self.capture.as_ref().unwrap().identity, editor);
            let capture = self.capture.take();
            self.retire(capture);
        }
        if self.ready.as_ref().is_some_and(|(identity, _)| !identity.matches(session, editor)) {
            Self::cancel_request(&self.ready.as_ref().unwrap().0, editor);
            let ready = self.ready.take();
            self.retire(ready);
        }
        if let Some(job) = self.job.as_mut() {
            if !job.cancelled && job.identity.as_ref().is_some_and(|identity| !identity.matches(session, editor)) {
                Self::cancel_request(job.identity.as_ref().unwrap(), editor);
                job.cancelled = true;
            }
            if let Some(output) = check_ready(&mut job.task) {
                let job = self.job.take().expect("上方已确认单飞任务存在");
                if let JobOutput::Prepared(result) = output {
                    if !job.cancelled && job.identity.as_ref().is_some_and(|identity| identity.matches(session, editor)) {
                        self.ready = Some((job.identity.expect("准备任务必须有完整身份"), result));
                    } else {
                        self.retire((job.identity, result)); // 换图后在途身份可能是整份历史的最后一个引用。
                    }
                }
            }
        }
    }

    /// 仅在原栈顶已经通过其他领域串行门后推进，不弹栈、不改删除票据。
    pub(super) fn prepare(&mut self, session: &EditorAuthoringSession, editor: &SceneEditorState,
        undoing: bool, command: &EditorCommand) -> Preparation {
        let Some(identity) = HistoryIdentity::new(session, undoing, command) else { return Preparation::Unneeded; };
        if !identity.matches(session, editor) || self.job.is_some() { return Preparation::Waiting; }
        if let Some((ready_identity, prepared)) = self.ready.take() {
            if ready_identity.matches(session, editor) {
                match prepared {
                    Err(error) => return Preparation::Failed(error),
                    Ok(prepared) if prepared.deletions.matches(&session.placement_deletions) => return Preparation::Ready(prepared),
                    Ok(prepared) => self.retire(prepared),
                }
            } else {
                self.retire((ready_identity, prepared));
            }
            return Preparation::Waiting;
        }
        let EditorCommand::CourtyardBatch(change) = command.unscoped() else { unreachable!(); };
        let capture = self.capture.get_or_insert_with(|| HistoryCapture {
            identity, cursor: 0, deleted: Vec::new(), restored: Vec::new(), records: Vec::new(),
        });
        if let Some(base) = change.wall_base {
            let draft = session.draft.as_ref().expect("已核对当前草稿身份");
            let Some(bounds) = draft.manifest.world_bounds() else {
                let capture = self.capture.take();
                self.retire(capture);
                return Preparation::Failed("恢复围墙时场景世界范围无效".into());
            };
            let complete = super::wall::capture(&mut capture.records, &mut capture.cursor, draft, change, base, undoing, CAPTURE_BUDGET);
            match complete {
                Err(error) => {
                    let capture = self.capture.take();
                    self.retire(capture);
                    return Preparation::Failed(error);
                }
                Ok(false) => return Preparation::Waiting,
                Ok(true) => {}
            }
            if !session.placement_deletions.write_shared() {
                let capture = self.capture.take().expect("已完成围墙摆放捕获");
                let source = session.placement_deletions.source();
                let change = change.clone();
                let scene = draft.scene_id.clone();
                let structure = session.placement_structure_revision;
                let water = draft.placement_water_surface_count;
                self.job = Some(PreparationJob { identity: Some(capture.identity), cancelled: false,
                    task: AsyncComputeTaskPool::get().spawn(async move {
                        JobOutput::Prepared(WallHistory::prepare(capture.records, water, change, source, scene,
                            bounds, structure, undoing))
                    }),
                });
            }
            return Preparation::Waiting;
        }
        let added = change.added.len();
        let total = added + change.removed.len();
        let end = (capture.cursor + CAPTURE_BUDGET).min(total);
        for index in capture.cursor..end {
            let (id, deleted) = if index < added {
                (&change.added[index].id, undoing)
            } else {
                (&change.removed[index - added].1.id, !undoing)
            };
            if deleted { capture.deleted.push(id.clone()); } else { capture.restored.push(id.clone()); }
        }
        capture.cursor = end;
        if end == total && !session.placement_deletions.write_shared() {
            let capture = self.capture.take().expect("刚完成完整ID捕获");
            let source = session.placement_deletions.source();
            self.job = Some(PreparationJob {
                identity: Some(capture.identity), cancelled: false,
                task: AsyncComputeTaskPool::get().spawn(async move {
                    let prepared = source.prepare(capture.deleted.iter().map(String::as_str), capture.restored.iter().map(String::as_str));
                    drop((capture.deleted, capture.restored));
                    JobOutput::Prepared(Ok(PreparedHistory::tickets(prepared)))
                }),
            }); // 整份删除表在后台构建，强共享源在完成值发布前释放。
        }
        Preparation::Waiting
    }

}

#[cfg(test)]
mod tests; // 真正CPU准备、取消背压、独立删除修订与普通历史共享门回归。
