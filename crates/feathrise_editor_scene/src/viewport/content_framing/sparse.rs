//! 后台读取稀疏作者高度，只保留一个磁盘块；未保存覆盖优先，禁止以编码量程冒充高度。
use super::types::{Bounds, SparseSource};
use bevy::prelude::*;
use feathrise_scene_content::{SparseHeightDocument, SparseHeightTile};
use std::path::Path;

pub(super) fn bounds(root: &Path, source: SparseSource) -> Result<Bounds, String> {
    let id = feathrise_scene_content::sparse_height_manifest_terrain_id(&source.scene_id, &source.path)?;
    let mut document =
        SparseHeightDocument::load_source(root, &source.scene_id, id.as_ref(), &source.path)?;
    let spec = document.manifest().spec.clone();
    // 缺席页代表真实Y=0；统计只访问非空作者页，绝不按逻辑整图分配。
    let mut low = f32::INFINITY;
    let mut high = f32::NEG_INFINITY;
    let mut occupied = source.live.len() as u64;
    let mut include = |tile: &SparseHeightTile| {
        for code in tile.height_codes(&spec) {
            let y = spec.decode_height(code);
            low = low.min(y);
            high = high.max(y);
        }
    };
    for i in 0..document.manifest().tiles.len() {
        let key = document.manifest().tiles[i].key;
        if source.live.contains_key(&key) {
            continue;
        }
        document.load_tile(key)?;
        include(document.tile_snapshot(key)?.as_ref());
        occupied += 1;
        document.unload_clean_tile(key); // 不让一次取景把全图高度常驻内存。
    }
    for tile in source.live.values() {
        include(tile);
    }
    let [width, length] = spec.tile_counts();
    if occupied < u64::from(width) * u64::from(length) {
        low = low.min(0.0);
        high = high.max(0.0);
    } // 只有缺席作者页才包含隐式零平面；完整抬高的网格不能被虚构的Y=0拉大取景。
    Ok(Bounds::transformed(
        Vec3::new(spec.world_min[0], low, spec.world_min[1]),
        Vec3::new(spec.world_max[0], high, spec.world_max[1]),
        source.frame,
    ))
}
