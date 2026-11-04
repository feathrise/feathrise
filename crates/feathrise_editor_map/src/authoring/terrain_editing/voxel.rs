//! 原生体素只读写笔刷相交的32³作者块；密度布尔操作复用体积核心，完整版本最后发布。
use super::{ObjectBrushInput, sources::read_source};
use crate::types::EditorTool;
use bevy::{
    prelude::*,
    tasks::{AsyncComputeTaskPool, IoTaskPool},
};
use feathrise_map_content::*;
use feathrise_map_terrain::volume::{VOLUME_QUANTIZATION, VolumeBrush, VolumeBrushKind};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

struct Input {
    source: MapVoxelTerrainSource,
    blocks: BTreeMap<[i32; 3], Vec<i16>>,
    min: IVec3,
    max: IVec3,
}
impl Input {
    fn sample(&self, point: IVec3) -> i16 {
        let key = point.to_array().map(|v| v.div_euclid(32));
        let local = point.to_array().map(|v| v.rem_euclid(32) as usize);
        self.blocks.get(&key).map_or_else(
            || box_value(&self.source, point),
            |block| block[(local[2] * 32 + local[1]) * 32 + local[0]],
        )
    }
}
pub(super) async fn edit(
    root: PathBuf,
    owner: MapId,
    mut object: MapTerrainObject,
    input: ObjectBrushInput,
    version: String,
) -> Result<MapTerrainObject, String> {
    let descriptor = object.voxel.clone().ok_or("体素对象缺少作者配置")?;
    let kind = match input.tool {
        EditorTool::VolumeDig | EditorTool::Lower => VolumeBrushKind::Dig,
        EditorTool::VolumeFill | EditorTool::Raise => VolumeBrushKind::Fill,
        EditorTool::VolumeSmooth | EditorTool::Smooth => VolumeBrushKind::Smooth,
        _ => VolumeBrushKind::Flatten,
    };
    let brush = VolumeBrush {
        kind,
        from: input.center.to_array(),
        to: input.center.to_array(),
        radius: input.radius,
        strength: input.strength,
        shape: input.shape,
        half_height: input.half_height,
        plane_height: input.plane_point.y,
        ..Default::default()
    };
    if !(0.1..=8.0).contains(&input.radius)
        || !input.half_height.is_finite()
        || input.half_height <= 0.0
    {
        return Err("原生体素单段笔刷半径为0.1～8m，半高必须为有限正数".into());
    }
    let matrix = object.transform.as_transform().to_matrix();
    let inverse = matrix.inverse();
    let world_cell = descriptor.cell_size_meters
        * object
            .transform
            .scale
            .into_iter()
            .fold(f32::INFINITY, f32::min);
    let half = brush.sample_half_extents(world_cell) + Vec3::splat(world_cell);
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for bits in 0..8 {
        let point = inverse.transform_point3(
            input.center
                + Vec3::new(
                    if bits & 1 == 0 { -half.x } else { half.x },
                    if bits & 2 == 0 { -half.y } else { half.y },
                    if bits & 4 == 0 { -half.z } else { half.z },
                ),
        );
        min = min.min(point);
        max = max.max(point);
    }
    let asset_root = root.clone();
    let expected = owner.clone();
    let id = object.id.clone();
    let (mut frozen, digest) = IoTaskPool::get()
        .spawn(async move {
            let (bytes, digest) =
                read_source(&asset_root, &descriptor.source_path, 32 * 1024 * 1024)?;
            let source: MapVoxelTerrainSource =
                serde_json::from_slice(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes))
                    .map_err(|e| e.to_string())?;
            source.validate()?;
            if source.map_id != expected
                || source.terrain_id != id
                || source.cell_size_meters != descriptor.cell_size_meters
                || source.min != descriptor.min
                || source.max != descriptor.max
            {
                return Err("体素作者源身份或规格已换代".into());
            }
            let lower = (min.max(Vec3::from_array(source.min)) / source.cell_size_meters)
                .floor()
                .as_ivec3();
            let upper = (max.min(Vec3::from_array(source.max)) / source.cell_size_meters)
                .ceil()
                .as_ivec3();
            let size = upper - lower + IVec3::ONE;
            if size.min_element() <= 0
                || size
                    .to_array()
                    .into_iter()
                    .try_fold(1u64, |n, v| n.checked_mul(v as u64))
                    .is_none_or(|n| n > 262_144)
            {
                return Err("体素单刷印超过262144格预算或位于对象外；请缩小笔刷".into());
            }
            let mut keys = BTreeSet::new();
            let first = (lower - IVec3::ONE).to_array().map(|v| v.div_euclid(32));
            let last = (upper + IVec3::ONE).to_array().map(|v| v.div_euclid(32));
            for z in first[2]..=last[2] {
                for y in first[1]..=last[1] {
                    for x in first[0]..=last[0] {
                        keys.insert([x, y, z]);
                    }
                }
            }
            if keys.len() > 36 {
                return Err("一次体素刷印最多预取36个局部块；请缩小笔刷".into());
            }
            let mut blocks = BTreeMap::new();
            for key in keys {
                if let Ok(index) = source
                    .blocks
                    .binary_search_by_key(&key, |reference| reference.key)
                {
                    let reference = &source.blocks[index];
                    let (bytes, hash) = read_source(
                        &asset_root,
                        &source.block_path(&reference.sha256),
                        32usize.pow(3) * 2,
                    )?;
                    if bytes.len() != 32usize.pow(3) * 2 || hash != reference.sha256 {
                        return Err("体素块长度或摘要无效".into());
                    }
                    blocks.insert(
                        key,
                        bytes
                            .chunks_exact(2)
                            .map(|v| i16::from_le_bytes([v[0], v[1]]))
                            .collect(),
                    );
                }
            }
            Ok::<_, String>((
                Input {
                    source,
                    blocks,
                    min: lower,
                    max: upper,
                },
                digest,
            ))
        })
        .await?;
    let computed = AsyncComputeTaskPool::get()
        .spawn(async move {
            let mut changes = BTreeMap::<[i32; 3], Vec<i16>>::new();
            for z in frozen.min.z..=frozen.max.z {
                for y in frozen.min.y..=frozen.max.y {
                    for x in frozen.min.x..=frozen.max.x {
                        let at = IVec3::new(x, y, z);
                        let position =
                            matrix.transform_point3(at.as_vec3() * frozen.source.cell_size_meters);
                        let value = frozen.sample(at);
                        let average = [
                            IVec3::X,
                            IVec3::NEG_X,
                            IVec3::Y,
                            IVec3::NEG_Y,
                            IVec3::Z,
                            IVec3::NEG_Z,
                        ]
                        .into_iter()
                        .map(|offset| frozen.sample(at + offset) as f32)
                        .sum::<f32>()
                            / 6.0;
                        if let Some(next) = brush.edit_density_sample(
                            value,
                            average,
                            position,
                            input.center,
                            world_cell,
                        ) && next != value
                        {
                            let key = at.to_array().map(|v| v.div_euclid(32));
                            let local = at.to_array().map(|v| v.rem_euclid(32) as usize);
                            let block = changes.entry(key).or_insert_with(|| {
                                frozen.blocks.get(&key).cloned().unwrap_or_else(|| {
                                    let mut values = Vec::with_capacity(32usize.pow(3));
                                    for z in 0..32 {
                                        for y in 0..32 {
                                            for x in 0..32 {
                                                values.push(box_value(
                                                    &frozen.source,
                                                    IVec3::from_array(key) * 32
                                                        + IVec3::new(x, y, z),
                                                ));
                                            }
                                        }
                                    }
                                    values
                                })
                            });
                            block[(local[2] * 32 + local[1]) * 32 + local[0]] = next;
                        }
                    }
                }
            }
            if changes.is_empty() {
                return Ok::<_, String>(None);
            }
            let mut refs = frozen
                .source
                .blocks
                .iter()
                .map(|reference| (reference.key, reference.sha256.clone()))
                .collect::<BTreeMap<_, _>>();
            let mut files = Vec::new();
            for (key, block) in changes {
                let bytes = block
                    .into_iter()
                    .flat_map(i16::to_le_bytes)
                    .collect::<Vec<_>>();
                let hash = authoring_io::bytes_hash(&bytes);
                files.push((frozen.source.block_path(&hash), bytes));
                refs.insert(key, hash);
            }
            frozen.source.blocks = refs
                .into_iter()
                .map(|(key, sha256)| MapVoxelDensityBlockRef { key, sha256 })
                .collect::<Vec<_>>()
                .into();
            let bytes = serde_json::to_vec_pretty(&frozen.source).map_err(|e| e.to_string())?;
            Ok(Some((files, bytes)))
        })
        .await?;
    if let Some((mut files, bytes)) = computed {
        let target = format!(
            "{}/versions/{version}.map_voxel_source.json",
            object.private_folder(&owner)
        );
        files.push((target.clone(), bytes));
        let original = object.voxel.as_ref().unwrap().source_path.clone();
        IoTaskPool::get()
            .spawn(async move {
                let source = crate::authoring::safe_asset_path(&root, &original)?;
                let mut writes = Vec::new();
                for (path, bytes) in files {
                    let target = crate::authoring::safe_asset_path(&root, &path)?;
                    let existing = authoring_io::file_hash(&target)?;
                    if existing == "missing" {
                        writes.push(authoring_transaction::PreparedWrite::new(target, bytes));
                    } else if existing != authoring_io::bytes_hash(&bytes) {
                        return Err("不可变体素版本路径已被其他作者占用".into());
                    }
                }
                authoring_transaction::commit_prepared_writes_checked(writes, move |_, target| {
                    authoring_io::compare_content_hash(
                        &digest,
                        &authoring_io::file_hash(&source)?,
                    )?;
                    authoring_io::compare_content_hash("missing", &authoring_io::file_hash(target)?)
                })
                .map_err(|e| e.message().to_owned())
            })
            .await?;
        object.voxel.as_mut().unwrap().source_path = target;
    }
    Ok(object)
}
fn box_value(source: &MapVoxelTerrainSource, point: IVec3) -> i16 {
    let p = point.as_vec3() * source.cell_size_meters;
    let min = Vec3::from_array(source.min);
    let max = Vec3::from_array(source.max);
    let d = (p - (min + max) * 0.5).abs() - (max - min) * 0.5;
    let distance = d.max(Vec3::ZERO).length() + d.max_element().min(0.0);
    (distance / source.cell_size_meters * VOLUME_QUANTIZATION)
        .round()
        .clamp(i16::MIN as f32, i16::MAX as f32) as i16
}
