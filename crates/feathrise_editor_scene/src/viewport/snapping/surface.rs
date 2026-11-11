//! 吸附后的真实承托层补查；只在后台使用BVH/Clipmap，洞内落点不回到外部高度。
use crate::authoring::PlacementTerrainSampler;
use bevy::prelude::*;
use feathrise_terrain::{TerrainEditingIndex, TerrainMeshInput};
use feathrise_scene::{ClipmapGroundHit, ClipmapGroundSnapshot};
use feathrise_editor_core::EditorTranslationSnap;

/// 高度场先发布吸附后的XZ种子，再由原Mip0完成门查询Y；三维层直接核验所选层。
pub(crate) fn clipmap_seed(
    surface: &ClipmapGroundSnapshot,
    hit: ClipmapGroundHit,
    snap: EditorTranslationSnap,
) -> ClipmapGroundHit {
    let ClipmapGroundHit::Hit(point) = hit else {
        return hit;
    };
    if !snap.active() {
        return hit;
    }
    let Some(snapped) = snap.horizontal(point.to_array()).map(Vec3::from_array) else {
        return ClipmapGroundHit::Failed;
    };
    if snapped.xz() == point.xz() {
        return hit;
    }
    if surface.is_three_dimensional_surface(point) {
        surface.sample_support_along_surface(point, snapped.xz())
    } else {
        let _ = surface.request_authoritative_rect(
            snapped.xz() - Vec2::splat(0.01),
            snapped.xz() + Vec2::splat(0.01),
        );
        ClipmapGroundHit::Hit(snapped)
    }
}

pub(crate) fn clipmap_hit(
    surface: &ClipmapGroundSnapshot,
    hit: ClipmapGroundHit,
    snap: EditorTranslationSnap,
) -> ClipmapGroundHit {
    let result = clipmap_seed(surface, hit, snap);
    let ClipmapGroundHit::Hit(point) = result else {
        return result;
    };
    if surface.is_three_dimensional_surface(point) {
        return result;
    }
    let _ = surface.request_authoritative_rect(
        point.xz() - Vec2::splat(0.01),
        point.xz() + Vec2::splat(0.01),
    );
    surface.sample_authoritative_surface(point.xz())
}

/// 普通高度场直接使用同代三角采样；任意GLB沿所选连续承托面追踪，上坡和断崖都不跳层。
pub(crate) fn mesh_hit(
    mesh: &TerrainMeshInput,
    index: &TerrainEditingIndex,
    point: Vec3,
    snap: EditorTranslationSnap,
    sampler: Option<&PlacementTerrainSampler>,
) -> ClipmapGroundHit {
    if !snap.active() {
        return ClipmapGroundHit::Hit(point);
    }
    let Some(mut snapped) = snap.horizontal(point.to_array()).map(Vec3::from_array) else {
        return ClipmapGroundHit::Failed;
    };
    if snapped.xz() == point.xz() {
        return ClipmapGroundHit::Hit(point);
    }
    if let Some(sampler) = sampler {
        return match sampler.sample(snapped.x, snapped.z) {
            Ok(height) => {
                snapped.y = height;
                ClipmapGroundHit::Hit(snapped)
            }
            Err(_) => ClipmapGroundHit::Failed,
        };
    }
    index.sample_connected_surface(&mesh.positions, &mesh.indices, point, snapped.xz())
        .map_or(ClipmapGroundHit::Miss, ClipmapGroundHit::Hit)
}

/// 独立对象按世界米制吸附，再沿冻结对象的真实连续表面补查；源姿态由同份查询处理。
pub(crate) fn object_hit(
    surface: &feathrise_scene::TerrainObjectRaySnapshot,
    point: Vec3,
    snap: EditorTranslationSnap,
) -> ClipmapGroundHit {
    if !snap.active() {
        return ClipmapGroundHit::Hit(point);
    }
    let Some(snapped) = snap.horizontal(point.to_array()).map(Vec3::from_array) else {
        return ClipmapGroundHit::Failed;
    };
    if snapped.xz() == point.xz() {
        return ClipmapGroundHit::Hit(point);
    }
    surface.sample_support_along_surface(point, snapped.xz())
}