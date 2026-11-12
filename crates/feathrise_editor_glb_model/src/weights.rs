//! 场景可见的蒙皮资料、不可变编辑请求和后台工作入口。

use crate::GlbTask;
use bevy::prelude::*;
use feathrise_editor_core::view_projection::ViewProjection;
pub use feathrise_editor_weights_model::types::Tool;
use std::{
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
};

pub const MAX_JOINTS: usize = 256;

const WIDTH: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VertexWeights {
    pub joints: [u16; WIDTH],
    pub weights: [f32; WIDTH],
}

impl VertexWeights {
    pub fn weight(self, joint: u16) -> f32 {
        self.joints
            .into_iter()
            .zip(self.weights)
            .find_map(|(index, weight)| (index == joint && weight > 0.0).then_some(weight))
            .unwrap_or(0.0)
    }
}

#[derive(Clone, Copy)]
pub struct MirrorPlane {
    pub center: Vec3,
    pub axis: Vec3,
}

impl MirrorPlane {
    pub fn reflect(self, point: Vec3) -> Vec3 {
        point - self.axis * (2.0 * (point - self.center).dot(self.axis))
    }
}

pub struct Joint {
    pub node: usize,
    pub name: String,
    /// 同一套蒙皮内最近的父关节，普通节点只参与中间变换。
    pub parent: Option<u16>,
    pub opposite: Option<u16>,
    pub rest: Vec3,
}

pub struct Skin {
    pub index: usize,
    pub joints: Vec<Joint>,
    pub mirror: Option<MirrorPlane>,
}

/// 已冻结 GLB 中的图元身份和原始权重。视口和后台引用同一份数据。
pub struct SurfaceInfo {
    pub mesh: usize,
    pub primitive: usize,
    pub name: String,
    pub skin: Arc<Skin>,
    pub original: Arc<[VertexWeights]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Geometry {
    Skin(usize),
    Rigid(usize),
}

pub enum PoseSample {
    Skin {
        surface: usize,
        matrices: Arc<[Mat4]>,
        weights: Arc<[VertexWeights]>,
    },
    Rigid(usize, Mat4),
}

#[derive(Clone)]
pub struct Brush {
    pub tool: Tool,
    pub joint: u16,
    pub radius: f32,
    pub strength: f32,
    pub visible_only: bool,
    pub mirror: bool,
    pub restrict_selection: bool,
    pub locks: Arc<[bool]>,
}

#[derive(Clone, Copy)]
pub struct Selection {
    pub view: ViewProjection,
    pub start: Vec2,
    pub end: Vec2,
    pub point: bool,
    pub extend: bool,
    pub visible: bool,
}

pub enum Operation {
    Refresh,
    Select(Selection, Option<Vec<PoseSample>>),
    Paint {
        view: ViewProjection,
        dabs: Vec<Vec2>,
        brush: Brush,
    },
    Set {
        joint: u16,
        amount: f32,
        locks: Arc<[bool]>,
    },
    History(Arc<[VertexWeights]>),
}

pub struct Prepared {
    pub surface: usize,
    pub weights: Arc<[VertexWeights]>,
    pub selection: Arc<[u32]>,
    pub shaded: Option<Mesh>,
    pub heat: Mesh,
    pub changed: usize,
}

pub struct EditRequest {
    pub surface: usize,
    pub before: Arc<[VertexWeights]>,
    pub selection: Arc<[u32]>,
    pub joint: u16,
    pub operation: Operation,
}

/// 冻结文档与空间索引由后台实现持有。场景拥有草稿、历史和唯一任务槽。
pub trait WeightDocument: Send + Sync {
    fn surface_count(&self) -> usize;
    fn surface(&self, index: usize) -> &SurfaceInfo;
    fn rigid_count(&self) -> usize;
    fn rigid_primitive(&self, index: usize) -> (usize, usize);
    fn rest_positions(&self, surface: usize) -> &[Vec3];
    fn rest_visible(&self, ray: Ray3d, point: Vec3, bias: f32) -> bool;
    fn prepare(&self, request: EditRequest, cancel: &AtomicBool) -> Result<Prepared, String>;
    fn save(
        self: Arc<Self>,
        root: PathBuf,
        draft: Vec<Arc<[VertexWeights]>>,
        cancel: Arc<AtomicBool>,
    ) -> GlbTask<String>;
}

impl dyn WeightDocument {
    pub fn surfaces(&self) -> impl ExactSizeIterator<Item = &SurfaceInfo> {
        (0..self.surface_count()).map(|index| self.surface(index))
    }
}

type Load = fn(PathBuf, String, Arc<AtomicBool>) -> GlbTask<Arc<dyn WeightDocument>>;

#[derive(Resource, Clone, Copy)]
pub struct WeightOperations {
    pub load: Load,
}
