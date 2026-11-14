//! 实窗失败取证冻结实际运行查询与作者BVH；三角摘要、射线和自有侧车写盘全部在唯一后台槽完成。
use super::{resources::GeometryFixture, types::Phase};
use crate::authoring::{EditorAuthoringSession, terrain_ground::TerrainGroundCursor};
use bevy::{ecs::system::RunSystemOnce, prelude::*, tasks::{AsyncComputeTaskPool, futures::check_ready}};
use feathrise_scene::{TerrainObjectQueries, TerrainObjectRaySnapshot};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{path::PathBuf, time::Duration};

/// 不等待后台完成、不写入命中或成功状态；原十秒Hover失败门保持不变。
pub(super) fn poll(world: &mut World, state: &mut GeometryFixture) {
    if let Some(result) = state.hover_query.as_mut().and_then(check_ready) {
        state.hover_query = None;
        let report = result.unwrap_or_else(|error| json!({"diagnostic_error":error}));
        eprintln!("实窗地形后台查询取证：{report}");
        if state.proof.hover_queries.len() < 4 { state.proof.hover_queries.push(report); }
    }
    if state.phase != Phase::Hover || state.phase_started.elapsed() < Duration::from_millis(250)
        || state.hover_query_captured || state.hover_query.is_some() { return; }
    let Some(scene) = state.scene.clone() else { return; };
    let Some(target) = state.target().cloned() else { return; };
    let Some(ray) = world.get_resource::<TerrainGroundCursor>().and_then(|cursor| cursor.ray) else { return; };
    let Some(ground) = world.resource::<EditorAuthoringSession>().draft.as_ref().and_then(|draft| draft.terrain_ground.clone()) else { return; };
    let hidden = world.resource::<crate::resources::SceneEditorState>().terrain_hidden.clone();
    let scene_for_query = scene.clone(); let target_for_query = target.clone();
    let actual = world.run_system_once(move |queries: TerrainObjectQueries| {
        let entity = queries.instance(&scene_for_query, &target_for_query)?;
        let (definition, query, version) = queries.snapshot_versioned(entity, &scene_for_query)?;
        Some((entity, definition, query, version.geometry_revision(),
            queries.voxel_diagnostic_snapshot(&scene_for_query, &target_for_query)))
    }).ok().flatten();
    let indexed = ground.object_query(&target);
    let directory = std::env::var_os("NEW_EDITOR_NATIVE_SMOKE_DIR").map(PathBuf::from);
    let domain = format!("{:?}", state.domain).to_lowercase();
    state.hover_query_captured = true;
    state.hover_query = Some(AsyncComputeTaskPool::get().spawn(async move {
        let trace = ground.validation_ray_trace(&target, ray.origin, ray.direction, &hidden);
        let mut report = json!({"scene":scene.as_str(),"terrain":target.as_str(),"domain":domain,
            "ray":{"origin":ray.origin.to_array(),"direction":ray.direction.to_array(),"epoch":ray.epoch},
            "ground":trace,"indexed_query":indexed.as_ref().map(|query| query_report(query,ray.origin,ray.direction))});
        if let Some((instance, definition, query, version, voxel)) = actual {
            report["instance"] = json!(instance.to_bits()); report["definition"] = json!(definition.as_ref());
            report["geometry_revision"] = json!(version); report["runtime_query"] = query_report(&query,ray.origin,ray.direction);
            if let Some((shape, frame, identity)) = voxel {
                report["voxel_identity"] = identity;
                let direct = TerrainObjectRaySnapshot::Mesh(shape.clone(),frame);
                report["display_query"] = query_report(&direct,ray.origin,ray.direction);
                if let Some(mesh) = shape.shape().as_trimesh() {
                    let bytes = mesh.vertices().len().saturating_mul(12).saturating_add(mesh.indices().len().saturating_mul(12));
                    if bytes > 32 * 1024 * 1024 { return Err("实际显示三角取证超过32MiB固定预算".into()); }
                    let positions: Vec<_> = mesh.vertices().iter().map(|point| point.to_array()).collect();
                    let indices = mesh.indices().to_vec();
                    let mut digest = Sha256::new();
                    for point in &positions { for value in point { digest.update(value.to_bits().to_le_bytes()); } }
                    for triangle in &indices { for index in triangle { digest.update(index.to_le_bytes()); } }
                    report["display_geometry"] = json!({"vertices":positions.len(),"triangles":indices.len(),
                        "sha256":digest.finalize().iter().map(|byte| format!("{byte:02x}")).collect::<String>(),"local_bounds":{
                            "min":shape.shape().compute_local_aabb().mins.to_array(),"max":shape.shape().compute_local_aabb().maxs.to_array()}});
                    if let Some(directory) = directory.as_ref().filter(|path| path.is_absolute()) {
                        // 只写原显式自有输出根；完整几何不进入stderr或主报告，避免截断前面的业务证据。
                        let name = format!("geometry-hover-{domain}.geometry.json");
                        let bytes = serde_json::to_vec(&json!({"scene":scene.as_str(),"terrain":target.as_str(),
                            "source_to_world":frame.source_to_world.to_cols_array(),"positions":positions,"indices":indices,
                            "ray":report["ray"],"voxel_identity":report["voxel_identity"]})).map_err(|error|error.to_string())?;
                        std::fs::write(directory.join(&name),bytes).map_err(|error|error.to_string())?;
                        report["display_geometry"]["file"] = json!(name);
                    }
                }
            }
        }
        if let Some(directory) = directory.filter(|path| path.is_absolute()) {
            std::fs::write(directory.join(format!("geometry-hover-{domain}.json")),
                serde_json::to_vec_pretty(&report).map_err(|error|error.to_string())?).map_err(|error|error.to_string())?;
        }
        Ok(report)
    }));
}

fn query_report(query: &TerrainObjectRaySnapshot, origin: Vec3, direction: Vec3) -> Value {
    let (variant, frame) = match query {
        TerrainObjectRaySnapshot::Clipmap(_,frame) => ("clipmap",frame),
        TerrainObjectRaySnapshot::Mesh(_,frame) => ("mesh",frame),
    };
    json!({"variant":variant,"source_to_world":frame.source_to_world.to_cols_array(),
        "world_to_source":frame.world_to_source.to_cols_array(),"local_origin":frame.to_local(origin).to_array(),
        "local_direction":frame.world_to_source.transform_vector3(direction).to_array(),
        "bounds":query.mesh_bounds().map(|(min,max)|json!({"min":min.to_array(),"max":max.to_array()})),
        "ray":query.ray(origin,direction).map(|point|point.to_array()),
        "surface":query.ray_surface(origin,direction,1_000_000.).map(|(point,normal)|json!({"point":point.to_array(),"normal":normal.to_array()}))})
}
