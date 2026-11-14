//! 保存前分帧捕获空气墙，其他领域保留现有Arc共享快照，绝不先克隆整图墙点。
use crate::authoring::resources::EditorSceneDraft;
use feathrise_scene_content::{SceneBoundariesAsset, SceneBoundaryCapture, SceneId};
#[derive(Default)]
pub(crate) struct BoundarySaveCapture {
    pub(super) identity: Option<(SceneId, u64)>, // 全部作者修订变化会重新开始。
    pub(super) capture: SceneBoundaryCapture,    // 4096点/64墙每帧。
}
pub(super) fn snapshot(
    source: &EditorSceneDraft,
    boundaries: Option<SceneBoundariesAsset>,
) -> EditorSceneDraft {
    EditorSceneDraft {
        scene_id: source.scene_id.clone(), // 保留该领域既有快照与共享语义。
        folder: source.folder.clone(), // 保留该领域既有快照与共享语义。
        water_asset_path: source.water_asset_path.clone(), // 保留该领域既有快照与共享语义。
        placement_glb_hash: source.placement_glb_hash.clone(),
        manifest: source.manifest.clone(), // 保留该领域既有快照与共享语义。
        terrain_ground: source.terrain_ground.clone(),
        terrain_documents: source.terrain_documents.clone(),
        placements: source.placements.clone(), // 保留该领域既有快照与共享语义。
        placement_water_surface_count: source.placement_water_surface_count.clone(), // 保留该领域既有快照与共享语义。
        portals: source.portals.clone(), // 保留该领域既有快照与共享语义。
        domain: source.domain.clone(), // 外部不可变版本浅共享。
        preserved_asset_paths: source.preserved_asset_paths.clone(),
        terrain_mesh: source.terrain_mesh.clone(), // 保留该领域既有快照与共享语义。
        terrain_editing_index: source.terrain_editing_index.clone(), // 保留该领域既有快照与共享语义。
        terrain_source_sha256: source.terrain_source_sha256.clone(), // 保留该领域既有快照与共享语义。
        terrain_overwrite_authorized_sha256: source.terrain_overwrite_authorized_sha256.clone(), // 保留该领域既有快照与共享语义。
        heights: source.heights.clone(), // 保留该领域既有快照与共享语义。
        surface: source.surface.clone(), // 保留该领域既有快照与共享语义。
        puddle_mask: source.puddle_mask.clone(), // 保存快照只共享完整R8，不在主线程复制画布。
        puddles_dirty: source.puddles_dirty, // 冻结本次积水raw及KTX2写入责任。
        surface_weights: source.surface_weights.clone(), // 保留该领域既有快照与共享语义。
        surface_indices: source.surface_indices.clone(), // 保留该领域既有快照与共享语义。
        vegetation_registration_source_hash: source.vegetation_registration_source_hash.clone(), // 保存冻结首次登记与磁盘冲突基线。
        vegetation: source.vegetation.clone(), // 保留该领域既有快照与共享语义。
        vegetation_densities: source.vegetation_densities.clone(), // 保留该领域既有快照与共享语义。
        vegetation_nonzero_layers: source.vegetation_nonzero_layers.clone(), // 保留该领域既有快照与共享语义。
        vegetation_density_stats: source.vegetation_density_stats.clone(), // 保留该领域既有快照与共享语义。
        vegetation_author_densities: source.vegetation_author_densities.clone(), // 保留该领域既有快照与共享语义。
        vegetation_conflicts: source.vegetation_conflicts.clone(), // 保留该领域既有快照与共享语义。
        water: source.water.clone(), // 保留该领域既有快照与共享语义。
        water_masks: source.water_masks.clone(), // 保留该领域既有快照与共享语义。
        boundaries,                  // 移动已经完整捕获的墙对象。
        terrain_dirty: source.terrain_dirty.clone(), // 保留该领域既有快照与共享语义。
        surface_dirty: source.surface_dirty.clone(), // 保留该领域既有快照与共享语义。
        vegetation_dirty_layers: source.vegetation_dirty_layers.clone(), // 保留该领域既有快照与共享语义。
        vegetation_author_dirty_layers: source.vegetation_author_dirty_layers.clone(), // 保留该领域既有快照与共享语义。
        water_dirty_layers: source.water_dirty_layers.clone(), // 保留该领域既有快照与共享语义。
        environment_dirty: source.environment_dirty.clone(),   // 保留该领域既有快照与共享语义。
        water_catalog_registration_required: source.water_catalog_registration_required.clone(), // 保留该领域既有快照与共享语义。
        water_registration_source_hash: source.water_registration_source_hash.clone(), // 首次水域登记冻结原文件存在状态与摘要。
        boundary_catalog_registration_required: source
            .boundary_catalog_registration_required
            .clone(), // 保留该领域既有快照与共享语义。
        boundaries_dirty: source.boundaries_dirty.clone(), // 保留该领域既有快照与共享语义。
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn save_snapshot_uses_captured_walls_and_keeps_source_unchanged() {
        let mut source =
            crate::authoring::test_heightfield_draft(std::sync::Arc::new(vec![0; 121]));
        let walls =
            SceneBoundariesAsset::rectangular_default(&source.scene_id, [0.0; 2], [10.0; 2], 0.0, 20.0)
                .unwrap();
        source.boundaries = Some(walls.clone());
        let mut capture = SceneBoundaryCapture::default();
        assert!(capture.step(source.boundaries.as_ref().unwrap(), 4096, 64));
        let saved = snapshot(&source, capture.take());
        assert_eq!(saved.boundaries.as_ref(), Some(&walls));
        assert_eq!(source.boundaries.as_ref(), Some(&walls)); // 不临时take或覆盖当前作者文档。
        assert!(std::sync::Arc::ptr_eq(
            source.heights.as_ref().unwrap(),
            saved.heights.as_ref().unwrap()
        )); // 大高度数组继续浅共享。
    }
}
