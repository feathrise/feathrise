//! 私有画布与作者源在IO池读取，几何派生在CPU池完成；失败不交换正在编辑的对象。
use super::super::files::{safe_asset_path, workspace_asset_root};
use super::{TerrainCanvas, TerrainDocumentIdentity};
use bevy::{prelude::*, tasks::AsyncComputeTaskPool};
use feathrise_map_content::*;
use std::{path::Path, sync::Arc};

/// 米制域与实例姿态不是源拓扑；同一几何的域重绑应保留已载入的精确作者顶点。
pub(super) fn same_heightfield_source(a: &MapTerrainObject, b: &MapTerrainObject) -> bool {
    a.id == b.id && a.terrain.kind == MapTerrainKind::Heightfield && b.terrain.kind == MapTerrainKind::Heightfield
        && a.terrain.glb_path == b.terrain.glb_path && a.terrain.resolution == b.terrain.resolution
        && a.world_min == b.world_min && a.world_max == b.world_max
}

pub(super) fn height_domain_rebind(a: &MapTerrainObject, b: &MapTerrainObject) -> bool {
    same_heightfield_source(a, b)
        && (a.terrain.base_height != b.terrain.base_height || a.terrain.height_scale != b.terrain.height_scale)
}

fn json<T: serde::de::DeserializeOwned>(
    root: &Path,
    path: Option<&str>,
) -> Result<Option<T>, String> {
    let Some(path) = path else {
        return Ok(None);
    };
    let target = safe_asset_path(root, path)?;
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(&target)
        .map_err(|e| format!("读取地形文档{path}失败：{e}"))?
        .take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err("地形文档超过16MiB读取预算".into());
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|e| format!("地形文档{path}格式无效：{e}"))
}
pub(crate) async fn load(
    mut identity: TerrainDocumentIdentity,
    cached: Option<Arc<TerrainCanvas>>,
    geometry_only: bool,
    saved: bool,
) -> Result<TerrainCanvas, String> {
    let object = identity.definition.clone();
    let root = workspace_asset_root();
    let mut canvas = if let Some(cached) = cached {
        Arc::try_unwrap(cached).unwrap_or_else(|shared| (*shared).clone())
    } else {
        TerrainCanvas::default()
    };
    let fresh = canvas.identity.is_none();
    if let Some(previous) = &canvas.identity {
        identity.manifest.puddles = previous.manifest.puddles.clone();
    }
    if saved {
        canvas.clear_dirty();
    }
    if (fresh || canvas.vegetation_only) && !geometry_only {
        canvas.surface = json(&root, object.surface_path.as_deref())?;
        if !canvas.vegetation_only { canvas.vegetation = json(&root, object.vegetation_path.as_deref())?; }
        canvas.water = json(&root, object.water_path.as_deref())?;
        canvas.water_asset_path = object
            .water_path
            .clone()
            .unwrap_or_else(|| format!("{}/{}", identity.folder, MAP_WATER_FILE_NAME));
        canvas.validate_paths(&root, &identity.folder)?; // 同MapId仍不得借用另一地形的像素源。
        if let Some(surface) = &canvas.surface {
            if surface.map_id != identity.manifest.id
                || surface.schema_version != MAP_SURFACE_SCHEMA_VERSION
            {
                return Err("地形地表文档的地图或版本不匹配".into());
            }
            let texels = surface.resolution[0] as usize * surface.resolution[1] as usize;
            if texels > 4096 * 4096 {
                return Err("地形地表画布超出4096边长预算".into());
            }
            canvas.surface_weights = Some(Arc::new(super::super::load::read_exact_bytes(
                &safe_asset_path(&root, &surface.weight_source_path)?,
                texels * 4,
                "地表权重",
            )?));
            canvas.surface_indices = Some(Arc::new(super::super::load::read_exact_bytes(
                &safe_asset_path(&root, &surface.index_source_path)?,
                texels * 16,
                "地表索引",
            )?));
        }
        if let Some(vegetation) = &canvas.vegetation && !canvas.vegetation_only {
            if vegetation.map_id != identity.manifest.id
                || vegetation.schema_version != MAP_SCHEMA_VERSION
            {
                return Err("地形植被文档身份不匹配".into());
            }
            if let Some(authoring) = &vegetation.authoring {
                authoring.validate(&vegetation.layers)?;
            }
            for layer in &vegetation.layers {
                let pixels = super::super::load::read_exact_bytes(
                    &safe_asset_path(&root, &layer.density_source_path)?,
                    layer.resolution[0] as usize * layer.resolution[1] as usize,
                    "植被密度",
                )?;
                canvas
                    .vegetation_densities
                    .insert(layer.id.clone(), Arc::new(pixels));
            }
            if let Some(authoring) = &vegetation.authoring {
                for layer in &authoring.layers {
                    let pixels = super::super::load::read_exact_bytes(
                        &safe_asset_path(&root, &layer.density_source_path)?,
                        layer.resolution[0] as usize * layer.resolution[1] as usize,
                        "植被作者密度",
                    )?;
                    canvas
                        .vegetation_author_densities
                        .insert(layer.id.clone(), Arc::new(pixels));
                }
            }
        }
        if let Some(water) = &canvas.water {
            water.validate(&identity.manifest.id)?;
            for layer in &water.layers {
                let pixels = super::super::load::read_exact_bytes(
                    &safe_asset_path(&root, &layer.mask_source_path)?,
                    layer.resolution[0] as usize * layer.resolution[1] as usize,
                    "水域覆盖",
                )?;
                canvas
                    .water_masks
                    .insert(layer.id.clone(), Arc::new(pixels));
            }
        }
        if let Some(puddles) = &identity.manifest.puddles {
            puddles.validate()?;
            super::super::files::safe_map_private_asset_path(
                &root,
                &identity.folder,
                &puddles.texture_path,
            )?;
            canvas.puddle_mask = Some(Arc::new(super::super::load::read_exact_bytes(
                &super::super::files::safe_map_private_asset_path(
                    &root,
                    &identity.folder,
                    &puddles.source_path,
                )?,
                puddles.resolution[0] as usize * puddles.resolution[1] as usize,
                "积水潜力",
            )?));
        }
        canvas.water_asset_path = object
            .water_path
            .clone()
            .unwrap_or_else(|| format!("{}/{}", identity.folder, MAP_WATER_FILE_NAME));
    }
    // 换源只更新几何，已有未保存画布保留；加载对象不会把旧磁盘像素覆盖到当前作者版本。
    let changed = saved || canvas.vegetation_only
        || canvas.identity.as_ref().is_none_or(|old| {
            old.definition.terrain != object.terrain || old.definition.voxel != object.voxel
        });
    if changed && object.terrain.kind == MapTerrainKind::Heightfield {
        let spec = feathrise_map_terrain::TerrainSampleSpec {
            resolution: object.terrain.resolution,
            world_min: object.world_min,
            world_max: object.world_max,
            base_height: object.terrain.base_height,
            height_scale: object.terrain.height_scale,
        };
        let retained = (!saved && !canvas.vegetation_only
            && canvas.identity.as_ref().is_some_and(|old| height_domain_rebind(&old.definition, &object)))
            .then(|| canvas.terrain_mesh.clone()).flatten();
        if let Some(mesh) = retained {
            // 撤销/重做及域缩放只重新派生采样编码，内存WIP和顶点身份均保留；不重新读旧GLB。
            let heights = AsyncComputeTaskPool::get().spawn(async move {
                feathrise_map_terrain::derive_terrain((*mesh).clone(), spec).map(|derived| derived.heights)
            }).await?;
            canvas.heights = Some(Arc::new(heights));
        } else {
            let bytes = crate::ui::terrain_objects::read_glb(&object.terrain.glb_path)?;
            let (mesh, index, heights, digest) = AsyncComputeTaskPool::get()
                .spawn(async move {
                    use sha2::{Digest, Sha256};
                    let digest = Sha256::digest(&bytes).into();
                    let mesh = feathrise_map_terrain::parse_dynamic_terrain_glb(&bytes)?;
                    let index = feathrise_map_terrain::build_terrain_editing_index(&mesh)?;
                    let heights = feathrise_map_terrain::derive_terrain(mesh.clone(), spec)?.heights;
                    Ok::<_, String>((mesh, index, heights, digest))
                })
                .await?;
            canvas.terrain_mesh = Some(Arc::new(mesh));
            canvas.terrain_editing_index = Some(Arc::new(index));
            canvas.heights = Some(Arc::new(heights));
            canvas.terrain_source_sha256 = Some(digest);
        }
    } else if changed {
        canvas.terrain_mesh = None;
        canvas.terrain_editing_index = None;
        canvas.heights = None;
        canvas.terrain_source_sha256 = None;
    }
    canvas.identity = Some(identity);
    canvas.vegetation_only = false;
    Ok(canvas)
}

/// 跨网格笔划不加载地表索引、GLB几何和水域整图，避免扩大本笔的工作集。
pub(crate) fn load_vegetation_canvas(identity: TerrainDocumentIdentity, cached: Option<Arc<TerrainCanvas>>, saved: bool) -> Result<TerrainCanvas, String> {
    let mut canvas=cached.map(|v|Arc::try_unwrap(v).unwrap_or_else(|v|(*v).clone())).unwrap_or_else(||TerrainCanvas {vegetation_only:true,..Default::default()});
    if saved {canvas.clear_dirty();}
    if canvas.identity.is_none() {
        let root=workspace_asset_root();
        canvas.vegetation=json(&root,identity.definition.vegetation_path.as_deref())?;
        if let Some(vegetation)=&canvas.vegetation {
            if vegetation.map_id!=identity.manifest.id || vegetation.schema_version!=MAP_SCHEMA_VERSION {return Err("跨网格植被文档身份不匹配".into());}
            if let Some(authoring)=&vegetation.authoring {authoring.validate(&vegetation.layers)?;}
            for layer in &vegetation.layers {
                let path=super::super::files::safe_map_private_asset_path(&root,&identity.folder,&layer.density_source_path)?;
                let pixels=super::super::load::read_exact_bytes(&path,layer.resolution[0] as usize*layer.resolution[1] as usize,"跨网格正式密度")?;
                if pixels.iter().any(|v|*v!=0) {canvas.vegetation_nonzero_layers.insert(layer.id.clone());}
                canvas.vegetation_densities.insert(layer.id.clone(),Arc::new(pixels));
            }
            // 跨网格画布沿用正式密度与作者密度，不再维护退役地图专用的派生基线。
            if let Some(authoring)=&vegetation.authoring {for layer in &authoring.layers {
                let path=super::super::files::safe_map_private_asset_path(&root,&identity.folder,&layer.density_source_path)?;
                canvas.vegetation_author_densities.insert(layer.id.clone(),Arc::new(super::super::load::read_exact_bytes(&path,
                    layer.resolution[0] as usize*layer.resolution[1] as usize,"跨网格作者密度")?));
            }}
        }
        canvas.validate_paths(&root,&identity.folder)?;
    }
    canvas.identity=Some(identity);
    Ok(canvas)
}

/// 对象初始化只建立零贡献画布；任何源都不覆盖，结构候选尚未采用时文件不影响活动地图。
pub(crate) fn prepare_sources(
    map: &MapId,
    object: &MapTerrainObject,
) -> Result<Vec<(String, Vec<u8>)>, String> {
    let folder = object.private_folder(map);
    let surface = super::super::create::new_map_surface_asset(map, &folder);
    let texels = surface.resolution[0] as usize * surface.resolution[1] as usize;
    let indices = vec![0; texels * 16];
    let weights = vec![0; texels * 4];
    let (index_texture, weight_texture) =
        super::super::ktx2::encode_surface_maps(&indices, &weights, surface.resolution, 0)?;
    let vegetation = MapVegetationAsset {
        schema_version: MAP_SCHEMA_VERSION,
        map_id: map.clone(),
        layers: Vec::new(),
        authoring: Some(super::super::vegetation_defaults::empty_vegetation_authoring()),
    };
    let water = super::super::water_defaults::default_water_asset(map);
    Ok(vec![
        (surface.index_source_path.clone(), indices),
        (surface.weight_source_path.clone(), weights),
        (surface.index_texture_path.clone(), index_texture),
        (surface.weight_texture_path.clone(), weight_texture),
        (
            format!("{folder}/{MAP_SURFACE_FILE_NAME}"),
            serde_json::to_vec_pretty(&surface).map_err(|e| e.to_string())?,
        ),
        (
            format!("{folder}/{MAP_VEGETATION_FILE_NAME}"),
            serde_json::to_vec_pretty(&vegetation).map_err(|e| e.to_string())?,
        ),
        (
            format!("{folder}/{MAP_WATER_FILE_NAME}"),
            serde_json::to_vec_pretty(&water).map_err(|e| e.to_string())?,
        ),
    ])
}
