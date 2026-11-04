//! 集合写侧、后端初始化与LOD校验在后台完成；新源先准备，地图作者记录最后采用。
use super::types::{Mutation, Prepared};
use bevy::{
    prelude::*,
    tasks::{AsyncComputeTaskPool, IoTaskPool},
};
use feathrise_map_content::*;
use std::path::PathBuf;
use std::sync::Arc;

pub(super) async fn prepare(
    owner: MapManifestAsset,
    objects: Arc<[MapTerrainObject]>,
    mutation: Mutation,
    creation: [f32; 8],
    serial: u64,
) -> Result<Prepared, String> {
    prepare_at(crate::authoring::workspace_asset_root(), owner, objects, mutation, creation, serial).await
}

/// 新源与配套画布共用显式资产根，供隔离后台创建回归使用；既有导入、刷印和拼接读取入口不变。
pub(super) async fn prepare_at(
    root: PathBuf,
    owner: MapManifestAsset,
    objects: Arc<[MapTerrainObject]>,
    mutation: Mutation,
    creation: [f32; 8],
    serial: u64,
) -> Result<Prepared, String> {
    let initialize_canvas = matches!(&mutation, Mutation::Add(_) | Mutation::ImportGlb(_));
    let owner_id = owner.id.clone();
    let stitching=matches!(&mutation,Mutation::StitchNearest(..)|Mutation::Unstitch(_));
    let imported = if let Mutation::ImportGlb(path) = &mutation {
        let path = path.clone();
        let bytes = IoTaskPool::get()
            .spawn(async move { super::import_glb::read(&path) })
            .await?;
        Some(
            AsyncComputeTaskPool::get()
                .spawn(async move { super::import_glb::bounds(&bytes) })
                .await?,
        )
    } else {
        None
    };
    let source_target = match &mutation {
        Mutation::Brush(id, _) | Mutation::Field(id, _) | Mutation::HeightDomain(id, ..) => Some(id),
        _ => None,
    };
    let edited = if let Some(id) = source_target {
        let id = id.clone();
        let objects = objects.clone();
        let object = AsyncComputeTaskPool::get()
            .spawn(async move {
                objects
                    .iter()
                    .find(|object| object.id == id)
                    .cloned()
                    .ok_or("所选网格源已失效")
            })
            .await?;
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_millis();
        let version = format!("edit-{stamp:x}-{serial:x}");
        Some(match &mutation {
            Mutation::HeightDomain(_, span, mesh) => {
                let span = *span;
                let mesh = mesh.clone();
                AsyncComputeTaskPool::get().spawn(async move {
                    super::height::set_domain(object, span, &mesh)
                }).await?
            }
            Mutation::Brush(_, input) => {
                crate::authoring::terrain_editing::edit(
                    crate::authoring::workspace_asset_root(),
                    owner.id.clone(),
                    object,
                    *input,
                    version,
                )
                .await?
            }
            Mutation::Field(_, input) => {
                crate::authoring::terrain_editing::edit_fields(
                    crate::authoring::workspace_asset_root(),
                    owner.id.clone(),
                    object,
                    input.clone(),
                    version,
                )
                .await?
            }
            _ => unreachable!("已核对源修改动作"),
        })
    } else {
        None
    };
    let stitched = if let Mutation::StitchNearest(id,blocked)=&mutation {
        Some(super::stitches::prepare(owner.id.clone(),objects.clone(),id.clone(),blocked.clone(),serial).await?)
    } else {None};
    let (prepared, source) = AsyncComputeTaskPool::get()
        .spawn(async move {
            let mut records = stitched.unwrap_or_else(||objects.to_vec()); // 写侧只在后台物化。
            let mut source = None;
            let selected = match mutation {
                Mutation::StitchNearest(id,_) => Some(id),
                Mutation::Unstitch(id) => {super::stitches::remove(&mut records,&id);Some(id)},
                Mutation::LodPolicy(id, policy) => {
                    if let Some(policy) = policy { policy.validate()?; }
                    records.iter_mut().find(|object| object.id == id)
                        .ok_or("网格已失效")?.mesh_lod = policy;
                    Some(id)
                }
                Mutation::Rename(id, name) => {
                    let name = name.trim();
                    if name.is_empty()
                        || name.chars().count() > 128
                        || name.chars().any(char::is_control)
                    {
                        return Err("网格名称须为1至128个可显示字符".into());
                    }
                    records
                        .iter_mut()
                        .find(|object| object.id == id)
                        .ok_or("网格已失效")?
                        .display_name = name.to_owned();
                    Some(id)
                }
                Mutation::Enabled(id, value) => {
                    records
                        .iter_mut()
                        .find(|object| object.id == id)
                        .ok_or("网格已失效")?
                        .enabled = value;
                    Some(id)
                }
                Mutation::Brush(id, _) | Mutation::Field(id, _) | Mutation::HeightDomain(id, ..) => {
                    *records
                        .iter_mut()
                        .find(|object| object.id == id)
                        .ok_or("网格已失效")? = edited.ok_or("作者源修改结果缺失")?;
                    Some(id)
                }
                Mutation::Transform(id, pose) => {
                    let object = records
                        .iter_mut()
                        .find(|object| object.id == id)
                        .ok_or("所选网格已失效")?;
                    pose.validate()?;
                    object.transform = pose;
                    Some(id)
                }
                Mutation::ImportGlb(path) => {
                    let stamp = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_err(|e| e.to_string())?
                        .as_millis();
                    let id = TerrainId::parse(format!("terrain-{stamp:x}-{serial:x}"))?;
                    let mut object =
                        new_object(&owner.id, id.clone(), MapTerrainKind::Gltf, creation)?;
                    super::import_glb::configure(
                        &mut object,
                        path,
                        imported.ok_or("GLB真实范围尚未读取")?,
                        creation,
                    )?;
                    records.push(object);
                    Some(id)
                }
                Mutation::Add(kind) => {
                    let stamp = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_err(|e| e.to_string())?
                        .as_millis();
                    let id = TerrainId::parse(format!("terrain-{stamp:x}-{serial:x}"))?;
                    let mut object = new_object(&owner.id, id.clone(), kind, creation)?;
                    let folder = object.private_folder(&owner.id);
                    source = Some(match kind {
                        MapTerrainKind::Heightfield => {
                            let bytes = feathrise_map_terrain::build_flat_terrain_glb(
                                feathrise_map_terrain::TerrainSampleSpec {
                                    resolution: object.terrain.resolution,
                                    world_min: object.world_min,
                                    world_max: object.world_max,
                                    base_height: object.terrain.base_height,
                                    height_scale: object.terrain.height_scale,
                                },
                                0.0,
                            )?;
                            NewSource::Glb(object.terrain.glb_path.clone(), bytes)
                        }
                        MapTerrainKind::SparseClipmap => {
                            let spec = SparseHeightSpec::new(
                                object.world_min,
                                object.world_max,
                                creation[7],
                                object.terrain.base_height,
                                object.terrain.height_scale,
                            )?;
                            object.terrain.resolution = spec.cells.map(|v| v as u32 + 1);
                            object.terrain.collision_lods =
                                default_collision_lods_for_resolution(object.terrain.resolution);
                            object.terrain.display_lods =
                                Some(default_display_lods_for_spec(&spec)?);
                            NewSource::Clipmap(owner.id.clone(), id.clone(), spec)
                        }
                        MapTerrainKind::Voxel => {
                            let descriptor =
                                object.voxel.as_ref().ok_or("体素初始化缺少作者配置")?;
                            let data = MapVoxelTerrainSource {
                                schema_version: 1,
                                map_id: owner.id.clone(),
                                terrain_id: id.clone(),
                                cell_size_meters: descriptor.cell_size_meters,
                                min: descriptor.min,
                                max: descriptor.max,
                                blocks: Default::default(),
                                surface_fields_path: None,
                            };
                            data.validate()?;
                            NewSource::Voxel(
                                format!("{folder}/source.map_voxel_source.json"),
                                serde_json::to_vec_pretty(&data).map_err(|e| e.to_string())?,
                            )
                        }
                        _ => return Err("添加操作只接受GLB、Clipmap或体素".into()),
                    });
                    records.push(object);
                    Some(id)
                }
                Mutation::Remove(id) => {
                    records.retain(|object| object.id != id);
                    None
                }
                Mutation::Value(id, field, value, display) => {
                    let object = records
                        .iter_mut()
                        .find(|object| object.id == id)
                        .ok_or("所选网格已失效")?;
                    set_value(object, field, value, display)?;
                    Some(id)
                }
                Mutation::AddLod(id, display) => {
                    let object = records
                        .iter_mut()
                        .find(|object| object.id == id)
                        .ok_or("所选网格已失效")?;
                    change_lods(object, display, None)?;
                    Some(id)
                }
                Mutation::RemoveLod(id, display, index) => {
                    let object = records
                        .iter_mut()
                        .find(|object| object.id == id)
                        .ok_or("所选网格已失效")?;
                    change_lods(object, display, Some(index))?;
                    Some(id)
                }
            };
            let records: Arc<[MapTerrainObject]> = records.into();
            let changed =
                owner.terrains.as_deref() != Some(records.as_ref()) || !owner.terrain.is_empty();
            let stitch_bytes=if stitching {
                owner.terrains.iter().flat_map(|o|o.iter()).chain(records.iter()).flat_map(|o|o.clipmap_stitches.iter())
                    .map(|s|std::mem::size_of::<ClipmapStitch>()+s.profile.samples.len()*4+s.id.len()+s.source_path.len()+s.peer_source_path.len()).sum()
            }else{0};
            let mut candidate = owner;
            candidate.terrain = MapTerrainDescriptor::empty();
            candidate.terrains = Some(records.clone());
            validate_terrain_objects(&candidate)?;
            for object in records
                .iter()
                .filter(|object| object.terrain.kind == MapTerrainKind::SparseClipmap)
            {
                let projected = object.source_manifest(&candidate);
                let spec = SparseHeightSpec::from_resolution(
                    object.world_min,
                    object.world_max,
                    object.terrain.resolution,
                    object.terrain.base_height,
                    object.terrain.height_scale,
                )?;
                if let Some(lods) = &projected.terrain.display_lods {
                    compile_display_lod_plan(&spec, lods)?;
                }
                feathrise_scene::validate_layered_collision_settings(
                    &spec,
                    &projected.terrain.collision_lods,
                )?;
            }
            let selected_index = selected
                .as_ref()
                .and_then(|id| records.iter().position(|object| &object.id == id));
            Ok::<_, String>((
                Prepared {
                    stitch_bytes,
                    changed,
                    objects: records,
                    selected,
                    selected_index,
                },
                source,
            ))
        })
        .await?;
    if let Some(source) = source {
        let root = root.clone();
        IoTaskPool::get()
            .spawn(async move {
                match source {
                    NewSource::Glb(path, bytes) | NewSource::Voxel(path, bytes) => {
                        let target = crate::authoring::safe_asset_path(&root, &path)?;
                        if target.exists() {
                            return Err("新网格作者路径已经存在，未覆盖原文件".into());
                        }
                        crate::authoring::atomic_write(&target, &bytes)
                    }
                    NewSource::Clipmap(map, id, spec) => {
                        let mut document =
                            SparseHeightDocument::create_for_terrain(&root, map, Some(id), spec)?;
                        document.save_dirty()
                    }
                }
            })
            .await?;
    }
    if initialize_canvas
        && let Some(object) = prepared
            .selected_index
            .and_then(|index| prepared.objects.get(index))
            .cloned()
    {
        let files = AsyncComputeTaskPool::get()
            .spawn(async move {
                crate::authoring::terrain_documents::prepare_sources(&owner_id, &object)
            })
            .await?;
        IoTaskPool::get()
            .spawn(async move {
                let files = files
                    .into_iter()
                    .map(|(path, bytes)| {
                        let target = crate::authoring::safe_asset_path(&root, &path)?;
                        if target.exists() {
                            return Err("新网格画布路径已经存在，未覆盖任何文件".to_string());
                        }
                        Ok((target, bytes))
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                crate::authoring::commit_owned_files(files).map_err(|error| error.to_string())
            })
            .await?;
    }
    Ok(prepared)
}

enum NewSource {
    Glb(String, Vec<u8>),
    Voxel(String, Vec<u8>),
    Clipmap(MapId, TerrainId, SparseHeightSpec),
}

fn new_object(
    map: &MapId,
    id: TerrainId,
    kind: MapTerrainKind,
    values: [f32; 8],
) -> Result<MapTerrainObject, String> {
    let super::creation::CreationSpec { min, max, resolution } = super::creation::validate(kind, &values)?;
    let folder = format!("maps/{map}/terrains/{}", id.as_str());
    let voxel = (kind == MapTerrainKind::Voxel).then(|| MapVoxelTerrainDescriptor {
        cell_size_meters: values[7],
        min: [min[0], -values[4], min[1]],
        max: [max[0], 0.0, max[1]],
        source_path: format!("{folder}/source.map_voxel_source.json"),
        display_lods: default_collision_lods(),
        collision_lods: default_collision_lods(),
    });
    Ok(MapTerrainObject {clipmap_stitches: Default::default(),
        id,
        display_name: match kind {
            MapTerrainKind::Gltf => "GLB网格",
            MapTerrainKind::Heightfield => "高度网格",
            MapTerrainKind::SparseClipmap => "Clipmap网格",
            _ => "体素网格",
        }
        .into(),
        enabled: true,
        mesh_lod: None, // 新网格默认继承全局策略，旧TerrainId和私有作者路径保持兼容。
        transform: TerrainObjectTransform {
            translation: values[..3].try_into().unwrap(),
            yaw_degrees: values[6],
            ..Default::default()
        },
        world_min: min,
        world_max: max,
        terrain: MapTerrainDescriptor {resolved_stitches: Default::default(),
            kind,
            glb_path: if kind == MapTerrainKind::Heightfield {
                format!("{folder}/terrain.glb")
            } else {
                String::new()
            },
            height_path: (kind == MapTerrainKind::SparseClipmap)
                .then(|| format!("{folder}/height_tiles.map_height_manifest.json")),
            resolution,
            base_height: -values[4] * 0.5,
            height_scale: values[4],
            ..MapTerrainDescriptor::empty()
        },
        voxel,
        surface_path: Some(format!("{folder}/{MAP_SURFACE_FILE_NAME}")),
        vegetation_path: Some(format!("{folder}/{MAP_VEGETATION_FILE_NAME}")),
        water_path: Some(format!("{folder}/{MAP_WATER_FILE_NAME}")),
        puddles: None,
    })
}

pub(super) fn field_value(object: &MapTerrainObject, field: usize, display: bool) -> Option<f32> {
    if field < 3 {
        return Some(object.transform.translation[field]);
    }
    if (3..6).contains(&field) {
        let extent = match field {
            3 => object.world_max[0] - object.world_min[0],
            4 => object
                .voxel
                .as_ref()
                .map_or(object.terrain.height_scale, |v| v.max[1] - v.min[1]),
            _ => object.world_max[1] - object.world_min[1],
        };
        return Some(extent * object.transform.scale[field - 3]);
    }
    if field == 6 {
        return Some(object.transform.yaw_degrees);
    }
    if field == 7 && object.terrain.kind == MapTerrainKind::Gltf {
        return None;
    }
    if field == 7 {
        return Some(object.voxel.as_ref().map_or(
            (object.world_max[0] - object.world_min[0]) / (object.terrain.resolution[0] - 1) as f32,
            |v| v.cell_size_meters,
        ));
    }
    let row = (field - 8) / 2;
    if let Some(voxel) = &object.voxel {
        let lod = if display {
            &voxel.display_lods
        } else {
            &voxel.collision_lods
        }
        .get(row)?;
        return Some(if field % 2 == 0 {
            lod.max_distance_meters
        } else {
            lod.spacing_multiplier as f32
        });
    }
    if object.terrain.kind != MapTerrainKind::SparseClipmap {
        return None;
    }
    if display {
        let lod = object.terrain.display_lods.as_ref()?.get(row)?;
        if field % 2 == 0 {
            lod.outer_half_extent_meters
        } else {
            Some(lod.spacing_multiplier as f32)
        }
    } else {
        let lod = object.terrain.collision_lods.get(row)?;
        Some(if field % 2 == 0 {
            lod.max_distance_meters
        } else {
            lod.spacing_multiplier as f32
        })
    }
}

fn set_value(
    object: &mut MapTerrainObject,
    field: usize,
    value: f32,
    display: bool,
) -> Result<(), String> {
    if !value.is_finite() {
        return Err("网格参数必须是有限数值".into());
    }
    if field < 3 {
        object.transform.translation[field] = value;
    } else if (3..6).contains(&field) {
        let old = field_value(object, field, display).ok_or("网格源尺寸失效")?;
        if value <= 0.0 {
            return Err("网格源尺寸必须大于零".into());
        }
        object.transform.scale[field - 3] *= value / old;
    } else if field == 6 {
        object.transform.yaw_degrees = value.rem_euclid(360.0);
    } else if field == 7 {
        return Err("作者格距属于源数据；整体大小使用尺寸字段调整".into());
    } else {
        let row = (field - 8) / 2;
        let spacing = if field % 2 == 1 {
            if value.fract() != 0.0
                || value < 1.0
                || value > 32768.0
                || !(value as u32).is_power_of_two()
            {
                return Err("LOD倍率必须为1、2、4、8等二次幂".into());
            }
            Some(value as u32)
        } else {
            None
        };
        if let Some(voxel) = &mut object.voxel {
            let lod = if display {
                &mut voxel.display_lods
            } else {
                &mut voxel.collision_lods
            }
            .get_mut(row)
            .ok_or("LOD行已失效")?;
            if let Some(spacing) = spacing {
                lod.spacing_multiplier = spacing;
            } else {
                lod.max_distance_meters = value;
            }
        } else if display {
            let lod = object.terrain.display_lods.as_mut()
                .and_then(|rows| rows.get_mut(row))
                .ok_or("显示LOD行已失效")?;
            if let Some(spacing) = spacing {
                lod.spacing_multiplier = spacing;
            } else if lod.outer_half_extent_meters.is_some() {
                lod.outer_half_extent_meters = Some(value);
            } else {
                return Err("末显示层固定覆盖此网格边界".into());
            }
        } else {
            let lod = object
                .terrain
                .collision_lods
                .get_mut(row)
                .ok_or("碰撞LOD行已失效")?;
            if let Some(spacing) = spacing {
                lod.spacing_multiplier = spacing;
            } else {
                lod.max_distance_meters = value;
            }
        }
    }
    object.transform.validate()
}

fn change_lods(object: &mut MapTerrainObject, display: bool, remove: Option<usize>) -> Result<(), String> {
    if let Some(voxel) = &mut object.voxel {
        let lods = if display {
            &mut voxel.display_lods
        } else {
            &mut voxel.collision_lods
        };
        change_distance_lods(lods, remove)
    } else if object.terrain.kind == MapTerrainKind::SparseClipmap && display {
        let lods = object.terrain.display_lods.as_mut()
            .ok_or("Clipmap网格缺少显式显示LOD配置")?;
        if remove.is_none() {
            let previous = lods
                .iter()
                .rev()
                .find_map(|lod| lod.outer_half_extent_meters)
                .unwrap_or(16.0);
            let last = lods.last_mut().ok_or("显示LOD为空")?;
            last.outer_half_extent_meters = Some(previous * 2.0);
            let multiplier = last.spacing_multiplier;
            lods.push(MapTerrainDisplayLod {
                outer_half_extent_meters: None,
                spacing_multiplier: multiplier,
            });
        } else {
            if lods.len() <= 1 { return Err("显示LOD至少保留一层".into()); }
            let index = remove.unwrap();
            if index >= lods.len() { return Err("所选显示LOD已失效".into()); }
            lods.remove(index); // 首层、中间和末层都按提交时真实下标删除，不重排其余作者参数。
            lods.last_mut().unwrap().outer_half_extent_meters = None; // 删尾后新末层仍完整覆盖网格边界。
        }
        Ok(())
    } else if object.terrain.kind == MapTerrainKind::SparseClipmap {
        change_distance_lods(&mut object.terrain.collision_lods, remove)
    } else {
        Err("该网格表示不使用原生分层LOD列表".into())
    }
}
fn change_distance_lods(lods: &mut Vec<MapCollisionLod>, remove: Option<usize>) -> Result<(), String> {
    if remove.is_none() {
        let last = lods.last().ok_or("LOD列表为空")?;
        lods.push(MapCollisionLod {
            max_distance_meters: last.max_distance_meters * 2.0,
            spacing_multiplier: last.spacing_multiplier,
        });
    } else {
        if lods.len() <= 1 { return Err("LOD至少保留一层".into()); }
        let index = remove.unwrap();
        if index >= lods.len() { return Err("所选LOD已失效".into()); }
        lods.remove(index); // 显示和碰撞使用独立列表；只删除本次冻结的用途和条目。
    }
    Ok(())
}
