//! 一次内容取景的有界捕获票据；大块只持有Arc，完成前不改变相机。
use bevy::{prelude::*, tasks::Task};
use feathrise_map_content::{MapId, SparseHeightTile, SparseHeightTileKey};
use feathrise_scene_core::residency::ResidentMapToken;
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Bounds {
    pub min: Vec3,
    pub max: Vec3,
}
impl Bounds {
    pub fn merge(target: &mut Option<Self>, next: Self) {
        if !next.min.is_finite() || !next.max.is_finite() || next.min.cmpgt(next.max).any() {
            return;
        }
        *target = Some(target.map_or(next, |old| Self {
            min: old.min.min(next.min),
            max: old.max.max(next.max),
        }));
    }
    pub fn transformed(min: Vec3, max: Vec3, matrix: Mat4) -> Self {
        let center = matrix.transform_point3((min + max) * 0.5);
        let half = (max - min) * 0.5;
        let extent = matrix.transform_vector3(Vec3::X).abs() * half.x
            + matrix.transform_vector3(Vec3::Y).abs() * half.y
            + matrix.transform_vector3(Vec3::Z).abs() * half.z;
        Self {
            min: center - extent,
            max: center + extent,
        }
    }
}

#[derive(Clone, PartialEq)]
pub(super) struct Identity {
    pub map: MapId,
    pub revision: u64,
    pub terrain_revision: u64,
    pub token: Option<ResidentMapToken>,
    pub index_revision: u64,
    pub content_revision: [u64; 4], // 热重载可原位复用Entity，必须同时核对运行时采用代次。
    pub content_layers: bevy::camera::visibility::RenderLayers, // 应用取景资格变更后旧捕获与后台结果同样失效。
    pub camera: (Vec3, f32, f32, f32, UVec2, bool),
}

pub(super) struct SparseSource {
    pub entity: Option<Entity>,
    pub map: MapId,
    pub path: String,
    pub frame: Mat4,
    pub live: BTreeMap<SparseHeightTileKey, Arc<SparseHeightTile>>,
    pub last: Option<SparseHeightTileKey>,
}
pub(super) struct Scan {
    pub identity: Identity,
    pub cursor: usize,
    pub bounds: Option<Bounds>,
    pub sources: Vec<SparseSource>,
    pub capturing: bool,
}

#[derive(Default)]
pub(crate) struct ContentFraming {
    pub(super) scan: Option<Scan>,
    pub(super) running: Option<Identity>,
    pub(super) task: Option<Task<Result<Option<Bounds>, String>>>,
    pub(super) requested: bool,
    pub(super) refocus: bool,
}
