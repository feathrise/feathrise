//! 后台把完整作者输入编码为生产资产格式；只返回内存字节，绝不调用文件保存或 Blender。

use super::capture::{CanvasCapture, Capture, WaterCapture};
use feathrise_scene_content::{
    embedded_snapshot::{SceneSnapshot, SparseHeightSnapshotTile, TerrainHeightSnapshot},
    *,
};
use feathrise_editor_assets::ktx2::{encode_r8, encode_surface_maps};
use std::{
    collections::BTreeMap,
    io::{self, Write},
    sync::Arc,
};

const MAX_SNAPSHOT_BYTES: usize = 128 * 1024 * 1024;
#[derive(Default)]
struct Files {
    values: BTreeMap<String, Arc<Vec<u8>>>,
    portable_paths: BTreeMap<String, String>,
    bytes: usize,
}

pub(super) fn encode(mut capture: Capture) -> Result<Vec<u8>, String> {
    let mut files = Files::default();
    let container = capture.manifest.terrains.is_some();
    if !container || capture.source.active.is_some() {
        encode_canvas(&capture.active, &mut files)?;
    }
    let root_water_path = capture.entry.water_path.as_deref();
    if container && let Some(path) = root_water_path {
        encode_water(&capture.root_water, path, &mut files)?;
    }
    if let Some((definition, _)) = &capture.root_vegetation {
        // 容器根的正式层与作者图只描述模型稀疏样本的规则，不产生二维编译密度。
        // 完整文档保留已载入的样本；地形画布仍由 encode_canvas 严格编码成套 R8/KTX2。
        json(&mut files, &format!("{}/{}", capture.entry.folder, SCENE_VEGETATION_FILE_NAME), definition)?;
    }

    // 容器定义与每张停放画布在后台合并；选择哪块地形不会决定哪些未保存画布进入游戏。
    if let Some(objects) = &capture.manifest.terrains {
        let mut objects = objects.to_vec();
        for canvas in &capture.stored {
            let Some(identity) = &canvas.identity else {
                continue;
            };
            let Some(object) = objects
                .iter_mut()
                .find(|object| object.id == identity.definition.id)
            else {
                continue;
            };
            let captured = CanvasCapture::from_stored(canvas)?;
            apply_canvas_paths(object, &captured);
            encode_canvas(&captured, &mut files)?;
        }
        if let Some(id) = &capture.source.active
            && let Some(object) = objects.iter_mut().find(|object| &object.id == id)
        {
            apply_canvas_paths(object, &capture.active);
        }
        capture.manifest.terrains = Some(objects.into());
        validate_terrain_objects(&capture.manifest)?;
    }
    if let Some(global) = &capture.global {
        json(&mut files, WORLD_ENVIRONMENT_ASSET_PATH, global)?;
    }
    for (path, bytes) in capture.extra_files {
        insert(&mut files, &path, bytes)?;
    }

    let mut root_heights = Vec::new();
    let mut root_height_revision = None;
    let mut height_overrides = Vec::new();
    let mut height_bytes = 0usize;
    for (terrain, heights) in capture.heights {
        let authoring_height_revision = Some(heights.revision());
        let mut tiles = Vec::new();
        let mut overflow = false;
        heights.visit_mip_tiles(32, SparseHeightTileKey { x: 0, z: 0 }, |key, tile| {
            if overflow {
                return;
            }
            let bytes = tile.encode_bytes();
            height_bytes = height_bytes.saturating_add(bytes.len());
            if height_bytes.saturating_add(files.bytes) > MAX_SNAPSHOT_BYTES {
                overflow = true;
                return;
            }
            tiles.push(SparseHeightSnapshotTile {
                key,
                bytes: Arc::new(bytes),
            });
        });
        if overflow {
            return Err("完整作者资产与高度页超过128MiB内存快照预算".into());
        }
        if let Some(terrain_id) = terrain {
            height_overrides.push(TerrainHeightSnapshot { terrain_id, authoring_height_revision, tiles });
        } else {
            root_heights = tiles;
            root_height_revision = authoring_height_revision;
        }
    }
    // R8/JSON 已在 files 中按正式路径成套提供，DTO density 槽保持空，避免重复传送同一像素数组。
    let snapshot = SceneSnapshot {
        entry: capture.entry,
        manifest: capture.manifest,
        placements: capture.placements,
        portals: capture.portals,
        boundaries: capture.boundaries,
        surface: (!container).then_some(capture.active.surface).flatten(),
        vegetation: if container { capture.root_vegetation.map(|(definition, _)| definition) } else { capture.active.vegetation },
        water: capture.root_water.document,
        terrain_id: None,
        vegetation_densities: BTreeMap::new(),
        water_masks: BTreeMap::new(),
        sparse_height_tiles: root_heights,
        authoring_height_revision: root_height_revision,
        height_overrides,
        asset_overrides: files.values,
    };
    let mut writer = BoundedWriter::new(MAX_SNAPSHOT_BYTES);
    serde_json::to_writer(&mut writer, &snapshot)
        .map_err(|error| format!("完整作者快照编码失败：{error}"))?;
    Ok(writer.bytes)
}

fn apply_canvas_paths(object: &mut SceneTerrainObject, canvas: &CanvasCapture) {
    object.puddles = canvas.manifest.puddles.clone();
    if canvas.surface.is_some() {
        object.surface_path = Some(format!("{}/{}", canvas.folder, SCENE_SURFACE_FILE_NAME));
    }
    if canvas.vegetation.is_some() {
        object.vegetation_path = Some(format!("{}/{}", canvas.folder, SCENE_VEGETATION_FILE_NAME));
    }
    if canvas.water.document.is_some() {
        object.water_path = Some(canvas.water_path.clone());
    }
}

fn encode_canvas(canvas: &CanvasCapture, files: &mut Files) -> Result<(), String> {
    if let Some(surface) = &canvas.surface {
        json(
            files,
            &format!("{}/{}", canvas.folder, SCENE_SURFACE_FILE_NAME),
            surface,
        )?;
        let weights = canvas.weights.as_ref().ok_or("地表缺少完整权重作者源")?;
        let indices = canvas
            .indices
            .as_ref()
            .ok_or("地表缺少完整局部索引作者源")?;
        let count = u32::try_from(surface.layers.len()).map_err(|_| "地表层数超出索引格式")?;
        // 在生产编码器复制基础级与构造 Mip 之前估算完整成对输出，避免先分配大缓冲再拒收。
        let textures = texture_estimate(surface.resolution, 20)?.saturating_add(2048);
        check_budget(
            files,
            indices
                .len()
                .saturating_add(weights.len())
                .saturating_add(textures),
        )?;
        let (index_texture, weight_texture) =
            encode_surface_maps(indices, weights, surface.resolution, count)?;
        insert(files, &surface.weight_source_path, weights.clone())?;
        insert(files, &surface.index_source_path, indices.clone())?;
        insert(
            files,
            &surface.weight_texture_path,
            Arc::new(weight_texture),
        )?;
        insert(files, &surface.index_texture_path, Arc::new(index_texture))?;
    }
    if let Some(vegetation) = &canvas.vegetation {
        encode_vegetation(vegetation, &canvas.densities, &canvas.folder, files)?;
    }
    encode_water(&canvas.water, &canvas.water_path, files)?;
    if let Some(puddles) = &canvas.manifest.puddles {
        let mask = canvas
            .puddles
            .as_ref()
            .ok_or("积水描述缺少对应完整R8作者源")?;
        plane("积水", puddles.resolution, mask.len())?;
        insert(files, &puddles.source_path, mask.clone())?;
        check_budget(files, texture_estimate(puddles.resolution, 1)?)?;
        insert(files, &puddles.texture_path, Arc::new(encode_r8(mask, puddles.resolution)?))?;
    }
    Ok(())
}

/// 地形画布的正式二维密度必须完整；编码和像素扫描只发生于 worker。
fn encode_vegetation(vegetation: &SceneVegetationAsset, densities: &BTreeMap<String, Arc<Vec<u8>>>, folder: &str, files: &mut Files) -> Result<(), String> {
        json(
            files,
            &format!("{folder}/{SCENE_VEGETATION_FILE_NAME}"),
            vegetation,
        )?;
        for layer in &vegetation.layers {
            let density = densities
                .get(&layer.id)
                .ok_or_else(|| format!("植被层 {} 缺少编译后的密度", layer.id))?;
            plane(&layer.id, layer.resolution, density.len())?;
            check_budget(files, texture_estimate(layer.resolution, 1)?)?;
            let source = if layer
                .density_source_path
                .to_ascii_lowercase()
                .ends_with(".png")
            {
                // RGB PNG 编码器需要先展开三通道；计入最坏无压缩载荷及行/文件开销。
                check_budget(files, density.len().saturating_mul(4).saturating_add(4096))?;
                Arc::new(crate::authoring::encode_density_png(
                    density,
                    layer.resolution,
                )?)
            } else {
                density.clone()
            };
            insert(files, &layer.density_source_path, source)?;
            check_budget(files, texture_estimate(layer.resolution, 1)?)?;
            insert(
                files,
                &layer.density_texture_path,
                Arc::new(encode_r8(density, layer.resolution)?),
            )?;
        }
    Ok(())
}

fn encode_water(water: &WaterCapture, path: &str, files: &mut Files) -> Result<(), String> {
    let Some(document) = &water.document else {
        return Ok(());
    };
    json(files, path, document)?;
    for layer in &document.layers {
        let mask = water
            .masks
            .get(&layer.id)
            .ok_or_else(|| format!("水层 {} 缺少完整遮罩", layer.id))?;
        plane(&layer.id, layer.resolution, mask.len())?;
        insert(files, &layer.mask_source_path, mask.clone())?;
        check_budget(files, texture_estimate(layer.resolution, 1)?)?;
        insert(
            files,
            &layer.mask_texture_path,
            Arc::new(encode_r8(mask, layer.resolution)?),
        )?;
    }
    Ok(())
}

fn plane(label: &str, size: [u32; 2], len: usize) -> Result<(), String> {
    if size.contains(&0) || u64::from(size[0]) * u64::from(size[1]) != len as u64 {
        return Err(format!("{label}的画布尺寸和字节数不一致"));
    }
    Ok(())
}

fn json(files: &mut Files, path: &str, value: &impl serde::Serialize) -> Result<(), String> {
    let mut writer = BoundedWriter::new(MAX_SNAPSHOT_BYTES.saturating_sub(files.bytes));
    serde_json::to_writer(&mut writer, value).map_err(|error| error.to_string())?;
    insert(files, path, Arc::new(writer.bytes))
}

fn check_budget(files: &Files, additional: usize) -> Result<(), String> {
    if files.bytes.saturating_add(additional) > MAX_SNAPSHOT_BYTES {
        return Err("完整作者资产编码预算超过128MiB；保留上一有效工程场景".into());
    }
    Ok(())
}

fn texture_estimate(mut size: [u32; 2], channels: usize) -> Result<usize, String> {
    if size.contains(&0) {
        return Err("纹理尺寸必须大于零".into());
    }
    let mut bytes = 1024usize; // KTX2 头、DFD 与每级对齐的保守固定上界。
    loop {
        bytes = bytes
            .saturating_add(
                (size[0] as usize)
                    .saturating_mul(size[1] as usize)
                    .saturating_mul(channels),
            )
            .saturating_add(32);
        if bytes > MAX_SNAPSHOT_BYTES {
            return Err("完整纹理Mip链超过128MiB编码预算".into());
        }
        if size == [1, 1] {
            return Ok(bytes);
        }
        size = [(size[0] / 2).max(1), (size[1] / 2).max(1)];
    }
}

fn insert(files: &mut Files, path: &str, bytes: Arc<Vec<u8>>) -> Result<(), String> {
    // 传输名始终是资产根内的规范相对文件；拒绝路径穿越和资源片段，不能借IPC获得文件写权限。
    if path.is_empty()
        || path.contains(['\\', ':', '#', '\0'])
        || path.split('/').any(|part| {
            part.is_empty() || part == "." || part == ".." || part.ends_with(['.', ' '])
        })
    {
        return Err(format!("快照资产路径无效：{path}"));
    }
    let portable = path.to_lowercase();
    if files
        .portable_paths
        .get(&portable)
        .is_some_and(|previous| previous != path)
    {
        return Err(format!("快照资产存在Windows大小写路径别名：{path}"));
    }
    if let Some(previous) = files.values.get(path) {
        if previous.as_slice() != bytes.as_slice() {
            return Err(format!("不同作者画布占用相同资产路径：{path}"));
        }
        return Ok(());
    }
    let total = files.bytes.saturating_add(bytes.len());
    if total > MAX_SNAPSHOT_BYTES {
        return Err("完整作者资产超过128MiB内存快照预算".into());
    }
    files.bytes = total;
    files.portable_paths.insert(portable, path.to_owned());
    files.values.insert(path.to_owned(), bytes);
    Ok(())
}

/// serde 的字节数组编码也计入传输预算，在分配超过上限之前失败并保留上一有效场景。
struct BoundedWriter {
    bytes: Vec<u8>,
    limit: usize,
}
impl BoundedWriter {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
        }
    }
}
impl Write for BoundedWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.bytes.len().saturating_add(bytes.len()) > self.limit {
            return Err(io::Error::other("完整作者快照超过128MiB传输预算"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoder_rejects_large_inputs_before_allocating_and_caps_serialization_during_write() {
        assert!(texture_estimate([u32::MAX, u32::MAX], 20).is_err());
        assert!(texture_estimate([0, 1], 1).is_err());
        let mut files = Files::default();
        files.bytes = MAX_SNAPSHOT_BYTES - 100;
        assert!(check_budget(&files, texture_estimate([2, 2], 1).unwrap()).is_err());
        let mut writer = BoundedWriter::new(3);
        assert!(serde_json::to_writer(&mut writer, &vec![1, 2, 3, 4]).is_err());
        assert!(
            writer.bytes.len() <= 3,
            "超限写入必须先拒绝，不能先扩容再检查"
        );
    }

    #[test]
    fn asset_alias_and_escaping_paths_cannot_silently_replace_another_canvas() {
        let mut files = Files::default();
        insert(&mut files, "scenes/a/test.r8", Arc::new(vec![1])).unwrap();
        insert(&mut files, "scenes/a/test.r8", Arc::new(vec![1])).unwrap();
        assert!(insert(&mut files, "scenes/a/test.r8", Arc::new(vec![2])).is_err());
        assert!(insert(&mut files, "scenes/A/test.r8", Arc::new(vec![1])).is_err());
        for path in [
            "../other",
            "D:/other",
            "/absolute",
            "scenes\\bad",
            "scenes/a#Scene0",
            "scenes/file.",
            "scenes/file ",
        ] {
            assert!(insert(&mut files, path, Arc::new(vec![])).is_err());
        }
        assert_eq!(files.values["scenes/a/test.r8"].as_slice(), &[1]);
    }
}
