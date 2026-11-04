//! 整组历史的删除表后台准备：有限捕获ID、单飞CPU槽与完整作者身份拒收。
//!
//! 【功能设计】历史Box留在原栈，不克隆完整记录；每帧只捕获128个增删ID。
//! 真正启动worker时才共享逻辑表，任务及迟到结果完全退役前不复用该槽。

use bevy::{prelude::*, tasks::{AsyncComputeTaskPool, Task, futures::check_ready}};
use feathrise_map_content::MapId;

use crate::{authoring::{EditorAuthoringSession, EditorCommand}, resources::MapEditorState};
use crate::authoring::placement_deletion::{PlacementDeletionIdentity, PreparedPlacementDeletions};

const ID_CAPTURE_BUDGET: usize = 128;

struct HistoryIdentity {
    map: MapId,
    document: PlacementDeletionIdentity,
    deletion: u64,
    author: u64,
    structure: u64,
    derived_pose: u64,
    undoing: bool,
    top: usize,
    undo_len: usize,
    redo_len: usize,
}

impl HistoryIdentity {
    fn new(session: &EditorAuthoringSession, undoing: bool, command: &EditorCommand) -> Option<Self> {
        let EditorCommand::CourtyardBatch(change) = command.unscoped() else { return None; };
        if change.added.is_empty() && change.removed.is_empty() { return None; }
        Some(Self {
            map: session.draft.as_ref()?.map_id.clone(),
            document: session.placement_deletions.identity(),
            deletion: session.placement_deletions.revision(),
            author: session.revision,
            structure: session.placement_structure_revision,
            derived_pose: session.placement_derived_pose_revision,
            undoing,
            top: change.as_ref() as *const _ as usize,
            undo_len: session.undo.len(),
            redo_len: session.redo.len(),
        })
    }

    fn matches(&self, session: &EditorAuthoringSession, editor: &MapEditorState) -> bool {
        if !editor.open || editor.close_requested || editor.map_transition_in_progress
            || editor.pending_map_transition.is_some() || session.saving
            || editor.selected_map_id.as_ref() != Some(&self.map)
            || session.draft.as_ref().is_none_or(|draft| draft.map_id != self.map)
            || !self.document.matches(&session.placement_deletions)
            || self.deletion != session.placement_deletions.revision()
            || self.author != session.revision || self.structure != session.placement_structure_revision
            || self.derived_pose != session.placement_derived_pose_revision
            || self.undo_len != session.undo.len() || self.redo_len != session.redo.len()
            || !(if self.undoing { editor.undo_requested } else { editor.redo_requested && !editor.undo_requested })
        { return false; }
        let command = if self.undoing { session.undo.last() } else { session.redo.last() };
        command.is_some_and(|command| match command.unscoped() {
            EditorCommand::CourtyardBatch(change) => change.as_ref() as *const _ as usize == self.top,
            _ => false,
        })
    }
}

struct IdCapture {
    identity: HistoryIdentity,
    cursor: usize,
    deleted: Vec<String>,
    restored: Vec<String>,
}

enum JobOutput { Prepared(Result<PreparedPlacementDeletions, String>), Retired }
struct PreparationJob {
    identity: Option<HistoryIdentity>, // None是同槽后台析构，完成前仍施加single-flight背压。
    cancelled: bool,
    task: Task<JobOutput>,
}

#[derive(Default)]
pub(crate) struct HistoryDeletionPreparation {
    capture: Option<IdCapture>,
    job: Option<PreparationJob>,
    ready: Option<(HistoryIdentity, Result<PreparedPlacementDeletions, String>)>,
}

pub(super) enum Preparation { Unneeded, Waiting, Ready(PreparedPlacementDeletions), Failed(String) }

impl HistoryDeletionPreparation {
    fn cancel_request(identity: &HistoryIdentity, editor: &mut MapEditorState) {
        if identity.undoing { editor.undo_requested = false; } else { editor.redo_requested = false; }
        editor.status = "整组历史来源已经变化，旧请求未应用；历史记录已保留".into();
    }

    fn retire<T: Send + 'static>(&mut self, value: T) {
        debug_assert!(self.job.is_none());
        self.job = Some(PreparationJob {
            identity: None, cancelled: false,
            task: AsyncComputeTaskPool::get().spawn(async move { drop(value); JobOutput::Retired }),
        });
    }

    /// 即使没有新的请求，也非阻塞排空原任务；取消不能通过丢句柄假装worker已停止。
    pub(super) fn maintain(&mut self, session: &EditorAuthoringSession, editor: &mut MapEditorState) {
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
                        self.retire(result); // 大表或错误输入只在后台析构，旧结果不触及作者数据和历史。
                    }
                }
            }
        }
    }

    /// 仅在原栈顶已经通过其他领域串行门后推进，不弹栈、不改删除票据。
    pub(super) fn prepare(&mut self, session: &EditorAuthoringSession, editor: &MapEditorState,
        undoing: bool, command: &EditorCommand) -> Preparation {
        let Some(identity) = HistoryIdentity::new(session, undoing, command) else { return Preparation::Unneeded; };
        if !identity.matches(session, editor) || self.job.is_some() { return Preparation::Waiting; }
        if let Some((ready_identity, result)) = self.ready.take() {
            if !ready_identity.matches(session, editor) {
                self.retire(result);
                return Preparation::Waiting;
            }
            return match result {
                Ok(prepared) if prepared.matches(&session.placement_deletions) => Preparation::Ready(prepared),
                Ok(prepared) => { self.retire(prepared); Preparation::Waiting }
                Err(error) => Preparation::Failed(error),
            };
        }
        let EditorCommand::CourtyardBatch(change) = command.unscoped() else { unreachable!(); };
        let capture = self.capture.get_or_insert_with(|| IdCapture {
            identity, cursor: 0, deleted: Vec::new(), restored: Vec::new(),
        });
        let added = change.added.len();
        let total = added + change.removed.len();
        let end = (capture.cursor + ID_CAPTURE_BUDGET).min(total);
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
                    JobOutput::Prepared(prepared)
                }),
            }); // 全部重建和容量预检都在worker；source强共享在完成值发布前释放。
        }
        Preparation::Waiting
    }

    pub(super) fn reject_prepared(&mut self, prepared: Option<PreparedPlacementDeletions>) {
        if let Some(prepared) = prepared { self.retire(prepared); }
    }
}

#[cfg(test)]
mod tests; // 真正CPU准备、取消背压、独立删除修订与普通历史共享门回归。
