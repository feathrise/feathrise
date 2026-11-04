//! 原作者源和历史的只读取证；整图摘要仅在single-flight后台任务计算。

use super::{resources::TerrainWorkflow, types::*};
use crate::authoring::{
    EditorAuthoringSession, EditorCommand, SurfacePreviewState, TerrainPreviewState,
};
use bevy::{
    prelude::*,
    tasks::{IoTaskPool, Task},
};
use sha2::{Digest, Sha256};
use std::sync::Arc;

enum Source {
    Mesh(String, Arc<feathrise_map_terrain::TerrainMeshInput>),
    Bytes(Vec<(String, Arc<Vec<u8>>)>),
}

/// 借当前完整对象源，任务结束即释放Arc，避免夹具长期阻止原地形唯一写侧。
pub(super) fn start(
    world: &World,
    state: &TerrainWorkflow,
) -> Result<Task<Result<Fingerprint, String>>, String> {
    let session = world.resource::<EditorAuthoringSession>();
    let draft = session.draft.as_ref().ok_or("地形验收草稿已卸载")?;
    let source = match state.domain {
        Domain::Height => Source::Mesh(
            draft.terrain_manifest().terrain.glb_path.clone(),
            draft.terrain_mesh.clone().ok_or("原GLB作者网格不存在")?,
        ),
        Domain::Surface => {
            let surface = draft.surface.as_ref().ok_or("原地表定义不存在")?;
            Source::Bytes(vec![
                (
                    surface.weight_source_path.clone(),
                    draft.surface_weights.clone().ok_or("地表权重源不存在")?,
                ),
                (
                    surface.index_source_path.clone(),
                    draft.surface_indices.clone().ok_or("地表索引源不存在")?,
                ),
            ])
        }
        Domain::Blade => {
            let vegetation = draft.vegetation.as_ref().ok_or("原植被定义不存在")?;
            let layer = vegetation
                .authoring
                .as_ref()
                .and_then(|author| {
                    author
                        .layers
                        .iter()
                        .find(|layer| Some(&layer.id) == state.blade_layer.as_ref())
                })
                .ok_or("原Blade作者层不存在")?;
            let output = vegetation
                .layers
                .iter()
                .find(|output| output.id == layer.output_layer_id)
                .ok_or("原Blade正式输出不存在")?;
            Source::Bytes(vec![
                (
                    layer.density_source_path.clone(),
                    draft
                        .vegetation_author_densities
                        .get(&layer.id)
                        .cloned()
                        .ok_or("Blade作者R8不存在")?,
                ),
                (
                    output.density_source_path.clone(),
                    draft
                        .vegetation_densities
                        .get(&output.id)
                        .cloned()
                        .ok_or("Blade正式R8尚未编译")?,
                ),
            ])
        }
    };
    Ok(IoTaskPool::get().spawn(async move { fingerprint(source) }))
}

fn hex(value: impl AsRef<[u8]>) -> String {
    value
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn fingerprint(source: Source) -> Result<Fingerprint, String> {
    let mut proof = Fingerprint::default();
    let mut combined = Sha256::new();
    match source {
        Source::Mesh(path, mesh) => {
            // GLB容器编码可变；这里核对完整世界顶点和三角索引的精确小端作者值。
            for position in &mesh.positions {
                for value in position {
                    combined.update(value.to_le_bytes());
                }
                proof.nonzero_samples += usize::from(position[1] != 0.);
            }
            for index in &mesh.indices {
                combined.update(index.to_le_bytes());
            }
            proof.bytes = mesh.positions.len() * 12 + mesh.indices.len() * 4;
            proof.sha256 = hex(combined.finalize());
            proof.files.push((
                format!("{path}#positions-f32le+indices-u32le"),
                proof.sha256.clone(),
            ));
        }
        Source::Bytes(values) => {
            for (path, pixels) in values {
                combined.update((pixels.len() as u64).to_le_bytes());
                combined.update(pixels.as_slice());
                proof.nonzero_samples += pixels.iter().filter(|&&value| value != 0).count();
                proof.bytes += pixels.len();
                proof
                    .files
                    .push((path, hex(Sha256::digest(pixels.as_slice()))));
            }
            proof.sha256 = hex(combined.finalize());
        }
    }
    if proof.bytes == 0 {
        return Err("原作者源为空，拒绝空操作验收".into());
    }
    Ok(proof)
}

pub(super) fn ready(world: &mut World, state: &TerrainWorkflow) -> bool {
    let session = world.resource::<EditorAuthoringSession>();
    let Some(draft) = session.draft.as_ref() else {
        return false;
    };
    if Some(&draft.map_id) != state.map.as_ref()
        || draft
            .terrain_documents
            .active
            .as_ref()
            .map(|active| &active.definition.id)
            != state.terrain.as_ref()
        || draft.terrain_documents.loading
    {
        return false;
    }
    match state.domain {
        Domain::Height => {
            world
                .resource::<TerrainPreviewState>()
                .ready_for(&draft.map_id, session.terrain_revision)
                && draft.terrain_ground.as_ref().is_some_and(|ground| {
                    ground.map == draft.map_id && ground.revision == session.terrain_revision
                })
                && draft
                    .terrain_documents
                    .active
                    .as_ref()
                    .is_some_and(|active| {
                        draft.manifest.terrains.as_ref().and_then(|objects| {
                            objects
                                .iter()
                                .find(|object| object.id == active.definition.id)
                        }) == Some(active.definition.as_ref())
                    })
        }
        Domain::Surface => {
            let preview = world.resource::<SurfacePreviewState>();
            preview.applied_map_id.as_ref() == Some(&draft.map_id)
                && preview.applied_terrain_id.as_ref() == state.terrain.as_ref()
                && preview.applied_revision == session.surface_revision
                && preview.applied_preset_revision
                    == world
                        .resource::<feathrise_map_content::SurfacePresetRegistry>()
                        .revision
                && preview.material.is_some()
                && preview.candidate.is_none()
        }
        Domain::Blade => blade_ready(world, state),
    }
}

/// 发布不等于显示：只认当前画布的活动实例、同代布局与像素回执及实际目标四窗。
fn blade_ready(world: &mut World, state: &TerrainWorkflow) -> bool {
    use bevy::ecs::system::SystemState;
    use feathrise_environment::vegetation::{VegetationBladeLayer, blade::GpuBladeDensityMap};
    use feathrise_scene::{DynamicVegetationPhase, DynamicVegetationStatus, TerrainObjectResource};

    let session = world.resource::<EditorAuthoringSession>();
    let Some(draft) = session.draft.as_ref() else {
        return false;
    };
    if session.vegetation_compiled_revision != session.vegetation_revision
        || session.vegetation_compiled_structure_revision != session.vegetation_structure_revision
    {
        return false;
    }
    let Some(live) = world.get_resource::<feathrise_scene::DynamicBladeDraft>() else {
        return false;
    };
    let Some(snapshot) = live.for_map(&draft.map_id) else {
        return false;
    };
    let Some(output) = state.blade_output.as_ref() else {
        return false;
    };
    if snapshot.source_revision != session.vegetation_revision
        || !snapshot.densities.contains_key(output)
    {
        return false;
    }
    let Some(layer) = snapshot
        .definition
        .layers
        .iter()
        .find(|layer| &layer.id == output)
    else {
        return false;
    };
    let Some(runtime_layer) = layer.runtime_layer_id else {
        return false;
    };
    let map = draft.map_id.clone();
    let Some(terrain) = state.terrain.as_ref() else {
        return false;
    };
    let layout = live.layout_revision_for_map(&map);
    let density = live.revision_for_map(&map);
    let mut access = SystemState::<TerrainObjectResource<DynamicVegetationStatus>>::new(world);
    let Ok(statuses) = access.get(world) else {
        return false;
    };
    let Some(status) = statuses.for_object(&map, terrain) else {
        return false;
    };
    if status.map_id.as_ref() != Some(&map)
        || status.phase != DynamicVegetationPhase::Ready
        || status.applied_draft_revision != layout
        || status.applied_density_revision != density
        || status.blade_layer_count == 0
        || status.blade_window_count < 4
    {
        return false;
    }
    let Some((instance, _)) = statuses.for_saved_object(&map, terrain) else {
        return false;
    };
    let mut query = world.query::<(Entity, &VegetationBladeLayer, &GpuBladeDensityMap)>();
    let mut bindings = Vec::new();
    for (entity, layer, binding) in query.iter(world) {
        if layer.0.0 == runtime_layer && visible_descendant(world, entity, instance) {
            bindings.push(binding.clone());
            if bindings.len() > 4 {
                return false;
            } // 本轮原默认Blade预设固定四档，不借兄弟/候选窗凑数。
        }
    }
    bindings.len() == 4 && bindings.iter().all(|binding| binding == &bindings[0])
}

fn visible_descendant(world: &World, mut entity: Entity, instance: Entity) -> bool {
    for _ in 0..16 {
        if world.get::<Visibility>(entity) == Some(&Visibility::Hidden) {
            return false;
        }
        if entity == instance {
            return true;
        }
        let Some(parent) = world.get::<ChildOf>(entity) else {
            return false;
        };
        entity = parent.parent();
    }
    false
}

/// 验证最新命令确属本笔非空修改；完整可逆性由随后原Undo/Redo和后台全源摘要证明。
pub(super) fn history(world: &World, state: &TerrainWorkflow) -> Result<(String, usize), String> {
    let session = world.resource::<EditorAuthoringSession>();
    let mut command = session.undo.last().ok_or("原笔划没有历史记录")?;
    for _ in 0..8 {
        if let EditorCommand::TerrainCanvas { id, command: inner } = command {
            if Some(id) != state.terrain.as_ref() {
                return Err("原笔划历史属于另一地形画布".into());
            }
            command = inner;
        } else {
            break;
        }
    }
    let result = match (state.domain, command) {
        (Domain::Height, EditorCommand::TerrainChanged(changes)) => (
            "TerrainChanged",
            changes
                .iter()
                .filter(|change| change.before != change.after)
                .count(),
        ),
        (Domain::Height, EditorCommand::TerrainObjectsChanged { before, after }) => {
            let find = |objects: &feathrise_map_content::MapTerrainCollection| {
                objects
                    .objects
                    .as_ref()
                    .and_then(|values| {
                        values
                            .iter()
                            .find(|object| Some(&object.id) == state.terrain.as_ref())
                    })
                    .map(|object| object.terrain.glb_path.clone())
            };
            let before = find(before).ok_or("原雕刻历史缺少开始网格")?;
            let after = find(after).ok_or("原雕刻历史缺少完成网格")?;
            if before == after {
                return Err("原雕刻历史未发布新GLB版本".into());
            }
            ("TerrainObjectsChanged", 1)
        }
        (
            Domain::Surface,
            EditorCommand::SurfaceChanged {
                weight_changes,
                index_changes,
            },
        ) => (
            "SurfaceChanged",
            weight_changes
                .iter()
                .chain(index_changes)
                .filter(|change| change.before != change.after)
                .count(),
        ),
        (Domain::Blade, EditorCommand::VegetationChanged { layer_id, changes })
            if Some(layer_id) == state.blade_layer.as_ref() =>
        {
            (
                "VegetationChanged",
                changes
                    .iter()
                    .filter(|change| change.before != change.after)
                    .count(),
            )
        }
        (Domain::Blade, EditorCommand::VegetationPaintChanged(change)) => {
            if change.canvases.len() != 1 {
                return Err("有限Blade笔划意外修改多个网格".into());
            }
            let (before, after) = state
                .terrain
                .as_ref()
                .and_then(|terrain| change.canvases.get(terrain))
                .ok_or("Blade历史缺少原网格")?;
            let layer = state
                .blade_layer
                .as_ref()
                .ok_or("Blade历史缺少作者层身份")?;
            let a = before.authors.get(layer).ok_or("Blade历史缺少开始源")?;
            let b = after.authors.get(layer).ok_or("Blade历史缺少完成源")?;
            if Arc::ptr_eq(a, b) {
                return Err("Blade原子历史未产生新版本".into());
            }
            ("VegetationPaintChanged", 1)
        }
        _ => return Err(format!("{:?}没有形成对应的原笔划历史", state.domain)),
    };
    if result.1 == 0 {
        return Err("原笔划历史为空差分".into());
    }
    Ok((result.0.into(), result.1))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn full_author_fingerprint_detects_index_only_change_and_keeps_pair_order() {
        let make = |a, b| {
            fingerprint(Source::Bytes(vec![
                ("weights".into(), Arc::new(vec![a])),
                ("indices".into(), Arc::new(vec![b])),
            ]))
            .unwrap()
        };
        assert_ne!(make(255, 0).sha256, make(255, 1).sha256);
        assert_ne!(make(1, 2).sha256, make(2, 1).sha256);
        assert_eq!(make(1, 2).sha256, make(1, 2).sha256);
        assert_eq!(make(0, 17).nonzero_samples, 1);
    }
}
