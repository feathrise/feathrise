//! 网格编辑后台只保留一项在途工作和最新请求。取消后旧任务结清前仍占用席位。

use super::{geometry::{WorldMesh, world_mesh}, selection::MeshSelection};
use crate::viewport::placement_move::MoveCamera;
use bevy::{prelude::*, tasks::{AsyncComputeTaskPool, Task, TaskPool, futures::check_ready}};
use feathrise_scene_content::ScenePlacementRecord;
use std::sync::Arc;

/// 原有 64 点路径经过定向测量，沿用该同步小预算；作者网格的容量由几何数据契约决定。
pub(super) const IMMEDIATE_VERTICES: usize = 64;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum JobKind {
    Enter,
    Subdivide,
    Motion,
    BoxSelection,
}

pub(super) enum Prepared {
    Source(Arc<WorldMesh>),
    Subdivision {
        record: ScenePlacementRecord,
        selected: MeshSelection,
        mesh: Arc<WorldMesh>,
        visual: Mesh,
    },
    Motion {
        record: ScenePlacementRecord,
        mesh: Arc<WorldMesh>,
        visual: Mesh,
        origin: Option<Vec3>,
    },
    Selection {
        selected: MeshSelection,
        camera: MoveCamera,
        viewport_size: Vec2,
    },
}

type Work = Box<dyn FnOnce() -> Result<Prepared, String> + Send + Sync>;
type Output = (u64, JobKind, Result<Prepared, String>);

#[derive(Default)]
pub(super) struct MeshEditJobs {
    ticket: u64,
    waiting: Option<JobKind>,
    next: Option<(u64, JobKind, Work)>,
    running: Option<Task<Output>>,
    retiring: Option<Task<()>>,
}

impl MeshEditJobs {
    pub(super) fn waiting(&self) -> Option<JobKind> {
        self.waiting
    }

    pub(super) fn cancel(&mut self) {
        self.ticket = self.ticket.wrapping_add(1);
        self.waiting = None;
        self.next = None;
    }

    pub(super) fn submit(
        &mut self,
        kind: JobKind,
        work: impl FnOnce() -> Result<Prepared, String> + Send + Sync + 'static,
    ) {
        self.ticket = self.ticket.wrapping_add(1);
        self.waiting = Some(kind);
        self.next = Some((self.ticket, kind, Box::new(work)));
        self.start();
    }

    pub(super) fn poll(&mut self) -> Option<(JobKind, Result<Prepared, String>)> {
        if let Some(retiring) = self.retiring.as_mut() {
            check_ready(retiring)?;
            self.retiring = None;
            self.start();
        }
        let (ticket, kind, result) = check_ready(self.running.as_mut()?)?;
        self.running = None;
        if ticket != self.ticket {
            // 迟到的大型空间索引与 Mesh 也在后台释放，归还内存后再启动下一笔。
            self.retiring = Some(AsyncComputeTaskPool::get().spawn(async move { drop(result) }));
            return None;
        }
        self.waiting = None;
        Some((kind, result))
    }

    fn start(&mut self) {
        if self.running.is_none() && self.retiring.is_none()
            && let Some((ticket, kind, work)) = self.next.take()
        {
            self.running = Some(AsyncComputeTaskPool::get_or_init(TaskPool::new)
                .spawn(async move { (ticket, kind, work()) }));
        }
    }
}

pub(super) fn prepare_world_mesh(record: &ScenePlacementRecord) -> Result<Arc<WorldMesh>, String> {
    world_mesh(record).map(Arc::new).ok_or_else(|| "数量、拓扑或实例变换无效".into())
}
