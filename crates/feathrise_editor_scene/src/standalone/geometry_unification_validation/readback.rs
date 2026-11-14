//! 从本轮自有工程真实源读取规格、密度与内容摘要；所有文件和样本遍历均在单飞 IO 槽。

use super::types::{GeometrySourceProof, GeometryUnificationProof};
use super::readback_density::{self, DensityInput};
use bevy::tasks::{IoTaskPool, Task};
use feathrise_scene_content::*;
use sha2::{Digest, Sha256};
use std::{collections::{BTreeMap, BTreeSet}, io::Read, path::{Path, PathBuf}};

#[cfg(test)]
#[path = "readback_tests.rs"]
mod tests;

pub(super) fn start(root: PathBuf, owner: SceneId, object: SceneTerrainObject,
    live: Option<SparseHeightLiveSnapshot>, density: Option<DensityInput>) -> Task<Result<GeometrySourceProof, String>> {
    IoTaskPool::get().spawn(async move { capture(&root, &owner, &object, live, density) })
}

pub(super) fn start_saved_height(root: PathBuf, owner: SceneId, object: SceneTerrainObject,
    expected: super::types::GeometryDensityProof) -> Task<Result<GeometrySourceProof, String>> {
    IoTaskPool::get().spawn(async move {
        let input = readback_density::from_saved(&root, &owner, &object, &expected)?;
        capture(&root, &owner, &object, None, Some(input))
    })
}

pub(super) fn source_bytes(root: &Path, path: &str) -> Result<Vec<u8>, String> {
    let path = crate::authoring::safe_asset_path(root, path)?;
    let mut bytes = Vec::new();
    std::fs::File::open(path).map_err(|e| e.to_string())?.take(32 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() > 32 * 1024 * 1024 { return Err("验收源超过32MiB读取上限".into()); }
    Ok(bytes)
}

fn capture(root: &Path, owner: &SceneId, object: &SceneTerrainObject,
    live: Option<SparseHeightLiveSnapshot>, density: Option<DensityInput>) -> Result<GeometrySourceProof, String> {
    let mut proof = GeometrySourceProof { terrain_id: object.id.as_str().into(),
        world_extent: [object.world_max[0]-object.world_min[0], object.world_max[1]-object.world_min[1]],
        ..Default::default() };
    let mut digest = Sha256::new();
    match object.terrain.kind {
        SceneTerrainKind::SparseClipmap => {
            proof.kind = "height_clipmap".into();
            proof.source_path = object.terrain.height_path.clone().ok_or("Clipmap作者源缺失")?;
            let mut document = SparseHeightDocument::load_source(root, owner, Some(&object.id), &proof.source_path)?;
            let spec = document.manifest().spec.clone();
            proof.cells = spec.cells;
            // 此入口只读 0.01m 和 4m 夹具；超大规格只走原预算门，不允许这里遍历整图。
            if spec.cells.into_iter().any(|count| count > 64) { return Err("验收读取只接受至多64格的自有小地形".into()); }
            let lods = object.terrain.display_lods.as_ref().ok_or("Clipmap显示LOD缺失")?;
            compile_display_lod_plan(&spec, lods)?;
            proof.display_lod_count = lods.len();
            digest.update(serde_json::to_vec(&spec).map_err(|e| e.to_string())?);
            let mut overrides = BTreeMap::new();
            if let Some(live) = &live {
                if live.scene_id() != Some(owner) || live.tile_count() > 4 { return Err("高度实窗快照身份或局部页预算错误".into()); }
                live.visit_changed_tiles(None, |key, tile| { overrides.insert(key, tile.clone()); });
            }
            // 只对至多65²个真实格点做规范摘要，隐式平地与撤销后显式零页必须具有同一内容身份。
            // 未保存雕刻来自同代live Arc；保存后另走无覆盖磁盘回读，不能把旧清单冒充当前笔划。
            for z in 0..=spec.cells[1] {
                for x in 0..=spec.cells[0] {
                    let (key, local) = spec.tile_for_grid_point(x, z).ok_or("高度格点超界")?;
                    document.load_tile(key)?;
                    let height = match overrides.get(&key) {
                        Some(tile) => tile.get_height(&spec, local),
                        None => document.sample_grid_encoded(x, z)?,
                    }.ok_or("高度取证遇到未完整载入的作者页")?;
                    digest.update(height.to_le_bytes());
                    proof.nonzero_samples += usize::from(f32::from_bits(height) != 0.);
                }
            }
        }
        SceneTerrainKind::Voxel => {
            use feathrise_terrain::volume::sparse::paged::{decode_paged_volume, decode_surface_fields,
                plan_surface_field_windows, read_surface_fields};
            proof.kind = "volume_voxel".into();
            proof.source_path = object.voxel.as_ref().ok_or("体积作者配置缺失")?.source_path.clone();
            let bytes = source_bytes(root, &proof.source_path)?;
            digest.update(&bytes);
            let source: SceneVoxelTerrainSource = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            source.validate()?;
            if source.scene_id != *owner || source.terrain_id != object.id { return Err("体积回读身份不一致".into()); }
            proof.cell_size_meters = source.cell_size_meters;
            proof.source_height_meters = source.max[1] - source.min[1];
            if source.min.into_iter().chain(source.max).any(|value| value.abs() > 4.) {
                return Err("体素专项只读取源原点四米内的小夹具".into());
            }
            if let Some(path) = source.surface_fields_path {
                let bytes = source_bytes(root, &path)?;
                digest.update(&bytes);
                let paged = decode_paged_volume(&bytes)?.ok_or("体积字段不是持久根")?;
                let objects = crate::authoring::safe_asset_path(root, &format!("{}/fields/objects", object.private_folder(owner)))?;
                // 固定27块窗口覆盖小夹具的全部正负轴；不能只读几个顶点漏掉负Z或底面字段。
                let plan = plan_surface_field_windows(&paged, &[[0.;3]], 0);
                let fields = decode_surface_fields(&paged, read_surface_fields(&objects, &paged, plan)?)?;
                let mut positive = Sha256::new(); let mut negative = Sha256::new();
                let mut positive_layers = BTreeSet::new(); let mut negative_layers = BTreeSet::new();
                for sample in fields.iter_samples() {
                    proof.surface_samples += 1;
                    proof.nonzero_samples += sample.vegetation.iter().filter(|(_, value)| *value > 0).count();
                    if sample.vegetation.is_empty() { continue; }
                    let bytes = serde_json::to_vec(sample).map_err(|error| error.to_string())?;
                    if sample.normal[1] > 0.9 {
                        proof.positive_samples += 1; positive.update(&bytes);
                        positive_layers.extend(sample.vegetation.iter().filter(|(_, weight)| *weight > 0).map(|(id, _)| id.clone()));
                    } else if sample.normal[1] < -0.9 {
                        proof.negative_samples += 1; negative.update(&bytes);
                        negative_layers.extend(sample.vegetation.iter().filter(|(_, weight)| *weight > 0).map(|(id, _)| id.clone()));
                    }
                }
                proof.opposite_surfaces_same_cell = fields.samples.keys().filter(|cell| {
                    fields.samples_at(cell).any(|sample| sample.normal[1] > 0.9 && !sample.vegetation.is_empty())
                        && fields.samples_at(cell).any(|sample| sample.normal[1] < -0.9 && !sample.vegetation.is_empty())
                }).count();
                proof.positive_sha256 = positive.finalize().iter().map(|b| format!("{b:02x}")).collect();
                proof.negative_sha256 = negative.finalize().iter().map(|b| format!("{b:02x}")).collect();
                proof.positive_layers = positive_layers.into_iter().collect();
                proof.negative_layers = negative_layers.into_iter().collect();
            }
        }
        _ => return Err("几何专项只读取高度/体积地形".into()),
    }
    if let Some(input) = density {
        let density = readback_density::capture(input)?;
        digest.update(serde_json::to_vec(&density).map_err(|error| error.to_string())?);
        proof.density = Some(density);
    }
    proof.sha256 = digest.finalize().iter().map(|b| format!("{b:02x}")).collect();
    Ok(proof)
}

/// 此入口由宿主后台验收调用，不注册为帧系统；只接受专项产生的固定数量小源。
pub(super) fn verify_saved(root: &Path, proof: &GeometryUnificationProof) -> Result<(), String> {
    if !proof.complete || proof.error.is_some() || !proof.overbudget_source_unchanged {
        return Err("几何专项缺少完整原业务证据".into());
    }
    proof.verify_volume_sides()?;
    proof.verify_height_vegetation()?;
    if !proof.height_density_saved_matches || proof.height_save_sequence.is_none_or(|sequence| sequence == 0) {
        return Err("小Clipmap缺少原保存回执和独立落盘密度回读".into());
    }
    let owner = SceneId::parse(&proof.scene_id)?;
    let path = format!("scenes/{owner}/{SCENE_MANIFEST_FILE_NAME}");
    let manifest: SceneManifestAsset = serde_json::from_slice(&source_bytes(root, &path)?).map_err(|error| error.to_string())?;
    let objects = manifest.terrains.as_ref().filter(|objects| objects.len() <= 8).ok_or("保存后几何夹具对象集合缺失或越界")?;
    for expected in [proof.tiny.as_ref(), proof.small.as_ref().map(|stroke| &stroke.after), proof.voxel_underside.as_ref().map(|stroke| &stroke.after)] {
        let expected = expected.ok_or("几何专项缺少微型/高度/体积源证据")?;
        let object = objects.iter().find(|object| object.id.as_str() == expected.terrain_id).ok_or("原保存丢失几何专项稳定对象")?;
        if capture(root, &owner, object, None, None)? != *expected {
            return Err(format!("原保存后的几何作者源与重做结果不同：{}", expected.terrain_id));
        }
    }
    let expected = &proof.height_vegetation.as_ref().ok_or("缺少Clipmap种植证据")?.after;
    let object = objects.iter().find(|object| object.id.as_str() == expected.terrain_id).ok_or("保存丢失种植Clipmap")?;
    let input = readback_density::from_saved(root, &owner, object, expected.density.as_ref().ok_or("缺少密度摘要")?)?;
    if capture(root, &owner, object, None, Some(input))? != *expected {
        return Err("保存后的Clipmap作者密度或正式R8与重做结果不同".into());
    }
    Ok(())
}
