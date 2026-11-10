//! 路径草稿操作与后台结果；结构变更只在工作槽空闲时受理。
use bevy::prelude::*;
use feathrise_terrain::volume::{VolumeBrush, VolumePathPlan};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub(super) enum Edit {
    Refresh,
    Add(Vec3),
    Delete(usize),
    Replace(usize, Vec3),
    Load(Arc<crate::authoring::SculptPathLibrary>, usize),
}
pub(super) struct BuiltPath {
    pub nodes: Arc<[[f32; 3]]>,
    pub selected: Option<usize>,
    pub plan: Result<Option<Arc<VolumePathPlan>>, String>,
    pub preview: Arc<[VolumeBrush]>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::viewport::spatial_brush) enum PathAction {
    Add,
    NewPoint,
    Delete,
    Previous,
    Next,
    Polyline,
    Curve,
    Rectangle,
    Round,
    Save,
    SavedPrevious,
    SavedNext,
    Load,
}

pub(super) struct IoResult {
    pub library: Result<Arc<crate::authoring::SculptPathLibrary>, String>,
    pub saved: bool,
}
