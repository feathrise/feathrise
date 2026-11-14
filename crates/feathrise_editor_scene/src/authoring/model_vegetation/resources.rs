//! 一个模型查询/采样槽、固定长度输入队列及整笔Arc前后值。

use bevy::{prelude::*, tasks::Task, world_serialization::WorldAsset};
use feathrise_scene::paint_surface::{MeshSurfaceSnapshot, PaintSurfaceHit};
use feathrise_scene_content::{ModelSurfaceVegetation, SceneId, ScenePlacementAssetTarget};
use std::{collections::VecDeque, sync::Arc};

pub(super) const MAX_QUEUED_SAMPLES: usize = 32;

#[derive(Clone, PartialEq)]
pub(super) struct Source {
    pub scene: SceneId,
    pub id: String,
    pub index: usize,
    pub path: String,
    pub target: ScenePlacementAssetTarget,
    pub epoch: u64,
}

#[derive(Clone, Copy)]
pub(super) struct Sample {
    pub ray: Ray3d,
    pub radius: f32,
    pub amount: f32,
    pub exposure: f32,
    pub erase: bool,
}

pub(super) struct Stroke {
    pub source: Source,
    pub matrix: Mat4,
    pub layer: String,
    pub data_index: usize,
    pub initialized: bool,
    pub sampled: bool,
    pub before_bytes: usize,
    pub after_bytes: usize,
    pub before: Arc<Vec<Arc<ModelSurfaceVegetation>>>,
    pub after: Option<Arc<Vec<Arc<ModelSurfaceVegetation>>>>,
    pub queue: VecDeque<Sample>,
    pub release: bool,
    pub last: Option<(Ray3d, PaintSurfaceHit)>,
}

pub(super) enum Work {
    Geometry(MeshSurfaceSnapshot),
    Paint {
        targets: Arc<Vec<Arc<ModelSurfaceVegetation>>>,
        changed: bool,
        last: Option<(Ray3d, PaintSurfaceHit)>,
        before_bytes: usize,
        after_bytes: usize,
    },
    Hover(Option<PaintSurfaceHit>),
}

pub(super) struct Pending {
    pub source: Source,
    pub matrix: Mat4,
    pub revision: u64,
    pub original: Option<Arc<Vec<Arc<ModelSurfaceVegetation>>>>,
    pub task: Task<Result<Work, String>>,
    pub painting: bool,
}

#[derive(Resource, Default)]
pub(crate) struct ModelVegetationState {
    pub(super) epoch: u64,
    pub(super) source: Option<Source>,
    pub(super) geometry: Option<MeshSurfaceSnapshot>,
    pub(super) handle: Option<Handle<WorldAsset>>,
    pub(super) pending: Option<Pending>,
    pub(super) retirement: Option<Task<()>>,
    pub(super) stroke: Option<Stroke>,
    pub(super) input_frame: bool,
    pub(super) failed: bool,
    /// 稳定帧只核对一个已找到的槽；第一次定位每帧最多检查64个模型目标。
    pub(super) scan: usize,
    pub(super) scan_done: bool,
    pub(super) data_index: Option<usize>,
    pub(super) structure_revision: u64,
    pub(super) hit: Option<PaintSurfaceHit>,
}

impl ModelVegetationState {
    pub(crate) fn busy(&self) -> bool {
        self.stroke.is_some()
            || self
                .pending
                .as_ref()
                .is_some_and(|pending| pending.painting)
    }

    #[cfg(feature = "validation")]
    pub(crate) fn cursor_hit(&self, object_id: &str) -> Option<Vec3> {
        self.source
            .as_ref()
            .filter(|source| source.id == object_id && source.epoch == self.epoch)?;
        self.hit.as_ref().map(|hit| hit.position)
    }
}
