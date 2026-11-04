//! 路径会话只共享冻结节点和预览；计算、IO及退役各保留真实在途句柄。
use super::types::*;
use bevy::{
    prelude::*,
    tasks::{AsyncComputeTaskPool, Task},
};
use feathrise_map_terrain::volume::{VolumeBrush, VolumePathParameters, VolumePathPlan};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Default)]
pub(crate) struct PathState {
    pub(crate) nodes: Arc<[[f32; 3]]>,
    pub(crate) selected: Option<usize>,
    pub(crate) parameters: VolumePathParameters,
    pub(crate) plan: Option<Arc<VolumePathPlan>>,
    pub(crate) preview: Arc<[VolumeBrush]>,
    pub(crate) error: Option<String>,
    pub(crate) library: Option<Arc<crate::authoring::SculptPathLibrary>>,
    pub(crate) saved_index: usize,
    pub(super) revision: u64,
    pub(super) working_revision: u64,
    pub(super) applied_revision: u64,
    pub(super) task: Option<Task<BuiltPath>>,
    pub(super) edit: Option<Edit>,
    pub(in crate::viewport::spatial_brush) observed_center: Option<Vec3>,
    pub(super) retire: Option<Task<()>>,
    pub(super) io: Option<Task<IoResult>>,
    pub(super) io_scope: Option<(String, String)>,
    pub(super) io_active: Arc<AtomicBool>,
    pub(super) library_attempted: bool,
}
impl PathState {
    pub(crate) fn busy(&self) -> bool {
        self.task.is_some() || self.edit.is_some() || self.retire.is_some()
    }
    pub(crate) fn ready(&self, center: Option<Vec3>) -> bool {
        !self.busy()
            && self
                .plan
                .as_ref()
                .is_some_and(|p| p.parameters() == self.parameters)
            && self.applied_revision == self.revision
            && self
                .selected
                .is_none_or(|i| self.nodes.get(i).map(|p| Vec3::from_array(*p)) == center)
    }
    pub(super) fn queue(&mut self, edit: Edit) {
        self.revision = self.revision.wrapping_add(1);
        self.edit = Some(edit);
        self.error = None;
    }
    pub(in crate::viewport::spatial_brush) fn visible_nodes(&self) -> [Option<usize>; 5] {
        if self.nodes.is_empty() {
            return [None; 5];
        }
        let last = self.nodes.len() - 1;
        let selected = self.selected.unwrap_or(last);
        let indices = [
            0,
            last,
            selected.saturating_sub(1),
            selected,
            (selected + 1).min(last),
        ];
        std::array::from_fn(|i| (!indices[..i].contains(&indices[i])).then_some(indices[i]))
    }
    pub(crate) fn reset(&mut self) {
        self.io_active.store(false, Ordering::Release);
        self.revision = self.revision.wrapping_add(1);
        self.edit = None;
        self.selected = None;
        self.observed_center = None;
        self.library_attempted = false;
        self.error = None;
        self.saved_index = 0;
        let old = (
            std::mem::take(&mut self.nodes),
            self.plan.take(),
            std::mem::take(&mut self.preview),
            self.library.take(),
        );
        if !old.0.is_empty() || old.1.is_some() || old.3.is_some() {
            // 一次退役由单飞槽完成；换图期间保留旧清理句柄，不丢弃正在运行的任务信用。
            let previous = self.retire.take();
            self.retire = Some(AsyncComputeTaskPool::get().spawn(async move {
                if let Some(previous) = previous {
                    previous.await;
                }
                drop(old);
            }));
        }
    }
    pub(super) fn release(&mut self, value: impl Send + 'static) {
        let previous = self.retire.take();
        self.retire = Some(AsyncComputeTaskPool::get().spawn(async move {
            if let Some(previous) = previous {
                previous.await;
            }
            drop(value);
        }));
    }
}
