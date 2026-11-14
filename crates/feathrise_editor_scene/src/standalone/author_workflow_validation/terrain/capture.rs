//! 原作者源和历史的只读取证；整图摘要仅在single-flight后台任务计算。

use super::{resources::TerrainWorkflow, types::*};
use crate::authoring::{
    EditorAuthoringSession, EditorCommand, SurfacePreviewState,
    sparse_terrain::SparseTerrainAuthoringState,
};
use bevy::{
    ecs::system::RunSystemOnce,
    prelude::*,
    tasks::{IoTaskPool, Task},
};
use feathrise_scene_content::{SparseHeightLiveOverride, SparseHeightLiveSnapshot};
use sha2::{Digest, Sha256};
use std::{collections::{BTreeMap, BTreeSet}, sync::Arc};

enum Source {
    Height {
        root: std::path::PathBuf,
        scene: feathrise_scene_content::SceneId,
        terrain: feathrise_scene_content::TerrainId,
        path: String,
        live: SparseHeightLiveSnapshot,
    },
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
        Domain::Height => Source::Height {
            root: feathrise_editor_core::workspace_asset_root(), scene: draft.scene_id.clone(),
            terrain: state.terrain.clone().ok_or("验收缺少高度地形身份")?,
            path: draft.terrain_manifest().terrain.height_path.clone().ok_or("Clipmap作者清单缺失")?,
            live: world.resource::<SparseHeightLiveOverride>().snapshot(),
        },
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
        Source::Height { root, scene, terrain, path, live } => {
            let mut document = feathrise_scene_content::SparseHeightDocument::load_source(&root, &scene, Some(&terrain), &path)?;
            let spec = document.manifest().spec.clone();
            if live.scene_id() != Some(&scene) || live.tile_count() > 16
                || document.manifest().tiles.len() > 16 || spec.cells.into_iter().any(|cells| cells > 64)
            {
                return Err("高度验收源身份或64格/16页预算不符".into());
            }
            let header = serde_json::to_vec(&spec).map_err(|error| error.to_string())?;
            combined.update(&header);
            proof.bytes = header.len();
            // 与几何专项一样，在后台叠加同代实时页。摘要沿用 F12 的已保存格式：有序非空页，而非全图逐点格式。
            let mut overrides = BTreeMap::new();
            live.visit_changed_tiles(None, |key, tile| { overrides.insert(key, tile.clone()); });
            let keys: BTreeSet<_> = document.manifest().tiles.iter().map(|tile| tile.key)
                .chain(overrides.keys().copied()).collect();
            for key in keys {
                let tile = if let Some(tile) = overrides.get(&key) {
                    if tile.is_flat(&spec) { continue; } // Undo 可留下显式零页，正式保存会把它归为隐式平地。
                    tile.clone()
                } else {
                    document.load_tile(key)?;
                    let tile = document.tile_snapshot(key)?;
                    document.unload_clean_tile(key);
                    tile
                };
                combined.update(key.x.to_le_bytes()); combined.update(key.z.to_le_bytes());
                for code in tile.height_codes(&spec) {
                    combined.update(code.to_le_bytes());
                    proof.nonzero_samples += usize::from(f32::from_bits(code) != 0.0);
                    proof.bytes += 4;
                }
            }
            proof.sha256 = hex(combined.finalize());
            proof.files.push((path, proof.sha256.clone()));
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
    if Some(&draft.scene_id) != state.scene_id.as_ref()
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
            let same_canvas = draft.terrain_ground.as_ref().is_some_and(|ground| {
                    ground.scene_id == draft.scene_id && ground.revision == session.terrain_revision
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
                    });
            same_canvas && world.run_system_once(|session: Res<EditorAuthoringSession>,
                sparse: Res<SparseTerrainAuthoringState>, live: Res<SparseHeightLiveOverride>,
                ground: feathrise_scene::TerrainObjectClipmapGround| {
                sparse.document_can_park() && sparse.current_height_snapshot(&session, &live, &ground).is_some()
            }).unwrap_or(false) // 作者光标快照与已采用的源路径、对象和实时页修订须同时就绪。
        }
        Domain::Surface => {
            let preview = world.resource::<SurfacePreviewState>();
            preview.applied_scene_id.as_ref() == Some(&draft.scene_id)
                && preview.applied_terrain_id.as_ref() == state.terrain.as_ref()
                && preview.applied_revision == session.surface_revision
                && preview.applied_preset_revision
                    == world
                        .resource::<feathrise_scene_content::SurfacePresetRegistry>()
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
    let Some(snapshot) = live.for_scene(&draft.scene_id) else {
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
    let map = draft.scene_id.clone();
    let Some(terrain) = state.terrain.as_ref() else {
        return false;
    };
    let layout = live.layout_revision_for_scene(&map);
    let density = live.revision_for_scene(&map);
    let mut access = SystemState::<TerrainObjectResource<DynamicVegetationStatus>>::new(world);
    let Ok(statuses) = access.get(world) else {
        return false;
    };
    let Some(status) = statuses.for_object(&map, terrain) else {
        return false;
    };
    if status.scene_id.as_ref() != Some(&map)
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
        (Domain::Height, EditorCommand::SparseTerrainChanged(changes)) => (
            "SparseTerrainChanged", changes.iter().filter(|change| change.before != change.after).count(),
        ),
        (Domain::Height, EditorCommand::TerrainChanged(changes)) => (
            "TerrainChanged",
            changes
                .iter()
                .filter(|change| change.before != change.after)
                .count(),
        ),
        (Domain::Height, EditorCommand::TerrainObjectsChanged { before, after }) => {
            let find = |objects: &feathrise_scene_content::SceneTerrainCollection| {
                objects
                    .objects
                    .as_ref()
                    .and_then(|values| {
                        values
                            .iter()
                            .find(|object| Some(&object.id) == state.terrain.as_ref())
                    })
                    .and_then(|object| object.terrain.height_path.clone())
            };
            let before = find(before).ok_or("原雕刻历史缺少开始网格")?;
            let after = find(after).ok_or("原雕刻历史缺少完成网格")?;
            if before == after {
                return Err("原雕刻历史未发布新分页高度版本".into());
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
    fn live_height_fingerprint_matches_undo_redo_and_the_formal_save() {
        use feathrise_scene_content::{SceneId, SparseHeightDocument, SparseHeightSpec, TerrainId};

        let serial = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("feathrise-author-height-proof-{}-{serial}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let scene = SceneId::parse("MAP-author-height-proof").unwrap();
        let terrain = TerrainId::parse("starter-ground").unwrap();
        let spec = SparseHeightSpec::new([-32.; 2], [32.; 2], 2., -80., 160.).unwrap();
        let (key, _) = spec.tile_for_grid_point(12, 12).unwrap();
        let mut author = SparseHeightDocument::create_for_terrain(&root, scene.clone(), Some(terrain.clone()), spec).unwrap();
        let path = author.manifest().asset_path();
        author.save_dirty().unwrap();
        let mut disk = SparseHeightLiveOverride::default();
        disk.retarget(Some(scene.clone()), 0);
        let mut live = disk.clone();
        let read = |source: &SparseHeightLiveOverride| fingerprint(Source::Height {
            root: root.clone(), scene: scene.clone(), terrain: terrain.clone(), path: path.clone(), live: source.snapshot(),
        }).unwrap();
        let before = read(&disk);
        author.set_grid_height(12, 12, 1.25).unwrap();
        live.publish_tiles(&scene, 1, [author.tile_snapshot(key).unwrap()]).unwrap();
        let after = read(&live);
        assert_eq!(after.nonzero_samples, 1);
        assert_ne!(after.sha256, before.sha256);
        assert_eq!(read(&disk).sha256, before.sha256, "作者实时页尚未改变磁盘");

        author.set_grid_height(12, 12, 0.).unwrap();
        live.publish_tiles(&scene, 2, [author.tile_snapshot(key).unwrap()]).unwrap();
        assert_eq!(read(&live).sha256, before.sha256, "撤销后的显式零页应等于原隐式平地");
        author.set_grid_height(12, 12, 1.25).unwrap();
        live.publish_tiles(&scene, 3, [author.tile_snapshot(key).unwrap()]).unwrap();
        assert_eq!(read(&live).sha256, after.sha256);
        author.save_dirty().unwrap();
        assert_eq!(read(&disk).files, after.files, "原保存重读采用同一源路径及完整高度指纹");
        drop(author);
        std::fs::remove_dir_all(root).unwrap(); // 只清理本用例独占创建的临时资产根。
    }

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
