//! 删除保存快照的单飞后台捕获；取消和切换场景保留任务槽，过期完整结果也在后台释放。

use bevy::tasks::{AsyncComputeTaskPool, Task, futures::check_ready};
use feathrise_scene_content::SceneId;

use crate::authoring::{
    AuthoringRetirement,
    placement_deletion::{PlacementDeletionSnapshot, PreparedDeletionSnapshot},
    resources::EditorAuthoringSession,
};

struct CapturedDeletionSnapshot {
    scene_id: SceneId,
    prepared: PreparedDeletionSnapshot,
}

enum CaptureTask {
    Capture {
        scene_id: SceneId,
        cancelled: bool,
        task: Task<PreparedDeletionSnapshot>,
    },
    Retire(Task<()>), // 独立工具未安装共享回收资源时，退役继续占用同一个槽。
}

#[derive(Default)]
pub(crate) struct DeletionSaveCapture {
    task: Option<CaptureTask>,
    prepared: Option<CapturedDeletionSnapshot>,
}

impl CapturedDeletionSnapshot {
    fn matches(&self, session: &EditorAuthoringSession) -> bool {
        session.draft.as_ref().is_some_and(|draft| draft.scene_id == self.scene_id)
            && self.prepared.matches(&session.placement_deletions)
    }
}

impl DeletionSaveCapture {
    /// 必须在保存入口的任何提前返回之前执行；没有请求或草稿时也要接收旧任务并退役。
    pub(super) fn poll(
        &mut self,
        session: &EditorAuthoringSession,
        requested: bool,
        retirement: Option<&mut AuthoringRetirement>,
    ) {
        if let Some(CaptureTask::Retire(task)) = self.task.as_mut() {
            if check_ready(task).is_some() { self.task = None; }
        }
        let completed = match self.task.as_mut() {
            Some(CaptureTask::Capture { scene_id: map, cancelled, task }) => {
                // 取消一旦发生就粘住；相同场景的新请求也不能复活已取消批次。
                *cancelled |= !requested || !session.draft.as_ref().is_some_and(|draft| draft.scene_id == *map);
                check_ready(task).map(|prepared| (map.clone(), *cancelled, prepared))
            }
            _ => None,
        };
        if let Some((map, cancelled, prepared)) = completed {
            self.task = None; // worker自然结束并已释放源强Arc后，才能归还捕获席位。
            let completed = CapturedDeletionSnapshot { scene_id: map, prepared };
            if !cancelled && completed.matches(session) {
                self.prepared = Some(completed);
            } else {
                self.retire(completed, retirement);
                return;
            }
        }
        if self.prepared.as_ref().is_some_and(|prepared| !requested || !prepared.matches(session)) {
            let obsolete = self.prepared.take().expect("已检查完整删除候选存在");
            self.retire(obsolete, retirement);
        }
    }

    /// 非空删除表仅O(1)共享输入；ID克隆及弱票数组构造全部由唯一CPU worker完成。
    pub(super) fn step(&mut self, session: &EditorAuthoringSession) -> bool {
        if session.placement_deletions.is_empty() { return true; }
        if self.prepared.as_ref().is_some_and(|prepared| prepared.matches(session)) { return true; }
        if self.task.is_some() || self.prepared.is_some() { return false; }
        let Some(draft) = session.draft.as_ref() else { return true; };
        let source = session.placement_deletions.source();
        self.task = Some(CaptureTask::Capture {
            scene_id: draft.scene_id.clone(),
            cancelled: false,
            task: AsyncComputeTaskPool::get().spawn(async move { source.snapshot() }),
        });
        false // 启动帧只保留请求，不在主线程等待或再次同步捕获。
    }

    /// 所有可能拒绝启动IO的门都通过后，直接移动完整弱票数组，不复制或提前释放整批。
    pub(super) fn take(&mut self, session: &EditorAuthoringSession) -> Option<Vec<PlacementDeletionSnapshot>> {
        if session.placement_deletions.is_empty() { return Some(Vec::new()); }
        if !self.prepared.as_ref().is_some_and(|prepared| prepared.matches(session)) { return None; }
        let mut completed = self.prepared.take().expect("已检查删除候选完整且身份匹配");
        Some(completed.prepared.take())
    }

    fn retire(&mut self, obsolete: CapturedDeletionSnapshot, retirement: Option<&mut AuthoringRetirement>) {
        if let Some(retirement) = retirement {
            retirement.push(obsolete); // 共享唯一后台回收器，入队不遍历8192项弱票或ID。
        } else {
            debug_assert!(self.task.is_none());
            self.task = Some(CaptureTask::Retire(AsyncComputeTaskPool::get().spawn(async move {
                drop(obsolete);
            }))); // 无共享资源的保存夹具/独立工具也有可靠单飞退役，不使用detach累积任务。
        }
    }
}

#[cfg(test)]
mod tests;
