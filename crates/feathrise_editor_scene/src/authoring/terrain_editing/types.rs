//! 一次刷印冻结世界米制参数与按压票据；后台不得重新读取可变UI。
use crate::types::EditorTool;
use bevy::prelude::*;
#[derive(Clone, Copy)]
pub(crate) struct ObjectBrushInput {
    pub stroke: u64,
    pub tool: EditorTool,
    pub center: Vec3,
    pub radius: f32,
    pub strength: f32,
    pub speed: f32,
    pub delta_seconds: f32,
    /// 冻结首次命中的完整世界点，倾斜网格不能仅用世界Y还原本地整平面。
    pub plane_point: Vec3,
    pub shape: feathrise_terrain::volume::VolumeBrushShape,
    pub half_height: f32,
}

/// 一段三维涂绘冻结真实表面及世界笔触；对象姿态换代后由同一作者票据拒收。
#[derive(Clone)]
pub(crate) struct NativeFieldInput {
    pub stroke: u64,
    pub brush: feathrise_terrain::volume::sparse::paged::VolumeFieldBrush,
    pub query: feathrise_scene::TerrainObjectRaySnapshot,
    pub ray: (Vec3, Vec3),
}
