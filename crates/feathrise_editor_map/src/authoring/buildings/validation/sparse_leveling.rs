//! 单栋Clipmap整平的IO/CPU流水；原作者源只读，完整候选交给原建筑采用门。
use super::*;
use crate::authoring::sparse_terrain::SparseTerrainAuthorLease;
use bevy::tasks::{IoTaskPool, Task};
use feathrise_map_content::{
    MAX_SPARSE_BRUSH_CANDIDATES, SparseHeightSpec, SparseTerrainAuthorSession,
    SparseTerrainHeightChange,
};
use std::sync::Arc;

/// IO读取与纯CPU平台规划串行复用原建筑任务槽，不额外排队或并行编辑作者源。
pub(super) fn start(
    mut result: BuildingValidationResult,
    index: Arc<BuildingSpatialIndex>,
    bounds: Rect,
    mut lease: SparseTerrainAuthorLease,
    snapshot: feathrise_scene::ClipmapGroundSnapshot,
) -> Task<BuildingValidationTaskOutput> {
    result.sparse_leveling = None; // 等待标记只允许启动一次；IO/平台失败须回显错误，不能逐帧重试同一坏源。
    let source_footprint = super::super::terrain_coordinates::source_quad(result.request.terrain_frame.as_ref(), result.footprint);
    IoTaskPool::get().spawn(async move {
        let preparation =
            grid_bounds(lease.author.spec(), source_footprint).and_then(|(min, max, padding)| {
                lease.author.verify_source_version()?;
                let spec = lease.author.spec();
                let world_min = spec.grid_point_world(min[0], min[1]).unwrap();
                let world_max = spec.grid_point_world(max[0], max[1]).unwrap();
                if snapshot.session_crater_intersects_rect(
                    Vec2::new(world_min[0] as f32, world_min[1] as f32),
                    Vec2::new(world_max[0] as f32, world_max[1] as f32),
                )? {
                    return Err("整平范围存在运行时浅坑，不能将会话地形写入作者平台".into());
                }
                lease.author.prefetch_grid_rect(min, max)?;
                Ok((min, max, padding))
            });
        let Ok((min, max, padding)) = preparation else {
            result.outcome = Err(preparation.unwrap_err());
            return BuildingValidationTaskOutput {
                result,
                lease: Some(lease),
            };
        };
        AsyncComputeTaskPool::get()
            .spawn(async move {
                let outcome = validate_geometry(
                    &result.request,
                    result.footprint,
                    result.entrance_clearance,
                    &index,
                    bounds,
                    None,
                )
                .and_then(|()| build(&mut lease.author, source_footprint, min, max, padding));
                match outcome {
                    Ok((edit, platform_height)) => {
                        let platform_height = super::super::terrain_coordinates::world_height(result.request.terrain_frame.as_ref(), platform_height);
                        let root_y = platform_height
                            - result.request.preset.foundation.model_base_y
                                * result.request.scale[1];
                        let binding = result.request.terrain_frame.as_ref().map(|frame|
                            feathrise_map_content::MapTerrainBinding::capture(frame.id.clone(), frame.transform,
                                feathrise_map_content::TerrainObjectTransform {
                                    translation:[result.request.world_xz.x,root_y,result.request.world_xz.y],
                                    yaw_degrees:result.request.yaw_degrees,scale:result.request.scale,
                                    ..Default::default()
                                })).transpose();
                        let terrain_binding = match binding {
                            Ok(binding) => binding,
                            Err(error) => {
                                result.outcome = Err(error);
                                return BuildingValidationTaskOutput { result, lease: Some(lease) };
                            }
                        };
                        result.outcome = Ok(ValidatedBuildingPlacement {
                            terrain_binding,
                            translation: Vec3::new(
                                result.request.world_xz.x,
                                root_y,
                                result.request.world_xz.y,
                            ),
                            minimum_ground: platform_height,
                            maximum_ground: platform_height,
                        });
                        result.sparse_leveling = Some(SparseBuildingLevelingState::Prepared(
                            Arc::new(SparseBuildingLevelingOutput { edit }),
                        ));
                    }
                    Err(error) => result.outcome = Err(error),
                }
                BuildingValidationTaskOutput {
                    result,
                    lease: Some(lease),
                }
            })
            .await
    })
}

/// 旋转占地外扩一格对角线作平台，再外扩一格过渡，包含承托三角全部顶点。
fn grid_bounds(
    spec: &SparseHeightSpec,
    footprint: feathrise_map_content::BuildingWorldQuad,
) -> Result<([u64; 2], [u64; 2], f32), String> {
    spec.validate()?;
    let padding = spec.actual_spacing_meters[0].hypot(spec.actual_spacing_meters[1]) as f32;
    let aabb = footprint.aabb();
    if !aabb.min.is_finite() || !aabb.max.is_finite() {
        return Err("建筑整平占地无效".into());
    }
    if !footprint.inside(Rect::from_corners(Vec2::from_array(spec.world_min), Vec2::from_array(spec.world_max))) {
        return Err("建筑地基超出当前承托对象的基础作者范围，不能只整平截断部分".into());
    }
    let mut min = [0; 2];
    let mut max = [0; 2];
    for axis in 0..2 {
        min[axis] = ((f64::from(aabb.min[axis] - 2. * padding) - f64::from(spec.world_min[axis]))
            / spec.actual_spacing_meters[axis])
            .floor()
            .clamp(0., spec.cells[axis] as f64) as u64;
        max[axis] = ((f64::from(aabb.max[axis] + 2. * padding) - f64::from(spec.world_min[axis]))
            / spec.actual_spacing_meters[axis])
            .ceil()
            .clamp(0., spec.cells[axis] as f64) as u64;
    }
    let count = (max[0] - min[0] + 1)
        .checked_mul(max[1] - min[1] + 1)
        .ok_or("整平格点预算溢出")?;
    if count > MAX_SPARSE_BRUSH_CANDIDATES {
        return Err("单栋整平超过262144个基础格点预算，请使用更小占地".into());
    }
    Ok((min, max, padding))
}

/// 米制高度决定只抬不削；历史保留原始位模式，不能对负高度的浮点位模式直接排序。
fn build(
    author: &mut SparseTerrainAuthorSession,
    footprint: feathrise_map_content::BuildingWorldQuad,
    min: [u64; 2],
    max: [u64; 2],
    padding: f32,
) -> Result<(feathrise_map_content::SparseTerrainPreparedEdit, f32), String> {
    let spec = author.spec().clone();
    let mut candidates = Vec::new();
    let mut platform = None::<f32>;
    for z in min[1]..=max[1] {
        for x in min[0]..=max[0] {
            let world = spec.grid_point_world(x, z).unwrap();
            let distance = super::super::leveling::distance_to_quad(
                Vec2::new(world[0] as f32, world[1] as f32),
                footprint,
            );
            if distance > 2. * padding {
                continue;
            }
            let before = author
                .sample_grid_encoded(x, z)?
                .ok_or("整平基础页尚未完整预取")?;
            let before_height = spec.decode_height(before);
            if !before_height.is_finite() {
                return Err("整平作者页包含非有限高度".into());
            }
            if distance <= padding {
                platform = Some(platform.map_or(before_height, |highest| highest.max(before_height)));
            }
            candidates.push((x, z, distance, before, before_height));
        }
    }
    let platform = platform.ok_or("建筑占地内没有基础地形承托格点")?;
    let mut changes = Vec::new();
    for (x, z, distance, before, before_height) in candidates {
        if before_height >= platform {
            continue;
        } // 外圈原有高点不削低。
        let factor = (1. - (distance - padding).max(0.) / padding).clamp(0., 1.);
        // 先用宽精度插值再编码，极端有限正负高度相减也不能溢出，更不能把bits差当作米数。
        let after_height = (f64::from(before_height)
            + (f64::from(platform) - f64::from(before_height)) * f64::from(factor))
            .min(f64::from(platform)) as f32;
        let after = spec.encode_height(after_height)?;
        if after != before {
            changes.push(SparseTerrainHeightChange {
                x,
                z,
                before,
                after,
            });
        }
    }
    Ok((
        author.prepare_changes_loaded(Arc::new(changes), true)?,
        platform,
    ))
}

#[cfg(test)]
mod tests;
