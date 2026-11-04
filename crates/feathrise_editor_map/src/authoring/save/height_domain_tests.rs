//! 高度域真实UI编辑后的同源保存往返；无可选Blender工具时完整保存GLB和作者内容。
use super::*;
use feathrise_map_content::{MapManifestAsset, MapVegetationAsset, MapWaterAsset, TerrainId};

#[test]
fn height_domain_dirty_save_roundtrip_keeps_same_glb_vertices_all_paint_and_new_manifest_domain() {
    let temporary = super::super::super::tests::SaveTransactionTestDirectory::new();
    let mut draft = crate::ui::terrain_objects::page_tests::height_domain_edited_draft_for_save();
    draft.manifest.schema_version = MAP_SCHEMA_VERSION;
    draft.placements.schema_version = MAP_SCHEMA_VERSION;
    draft.portals.schema_version = MAP_SCHEMA_VERSION;
    let id = TerrainId::parse("mesh-a").unwrap();
    let current = draft.manifest.terrains.as_ref().unwrap().iter().find(|object| object.id == id).unwrap().clone();
    let mesh = draft.terrain_mesh.clone().unwrap();
    let glb_path = temporary.path.join(&current.terrain.glb_path);
    std::fs::create_dir_all(glb_path.parent().unwrap()).unwrap();
    let spec = feathrise_map_terrain::TerrainSampleSpec {
        resolution: current.terrain.resolution, world_min: current.world_min, world_max: current.world_max,
        base_height: current.terrain.base_height, height_scale: current.terrain.height_scale,
    };
    let baseline = feathrise_map_terrain::build_flat_terrain_glb(spec, 0.0).unwrap();
    std::fs::write(&glb_path, &baseline).unwrap();
    draft.terrain_source_sha256 = Some(Sha256::digest(&baseline).into());

    let (manifest, projections, saved) = bevy::tasks::block_on(
        super::super::super::terrain_documents::prepare(&draft)).unwrap();
    assert_eq!(saved, vec![id.clone()]);
    assert_eq!(projections.len(), 1);
    assert!(projections[0].terrain_dirty && projections[0].surface_dirty);
    assert!(Arc::ptr_eq(projections[0].terrain_mesh.as_ref().unwrap(), &mesh));
    assert_eq!(projections[0].manifest.terrain.height_scale, 40.0, "保存投影必须读取真实新域");
    let manifest = manifest.unwrap();
    let manifest_path = temporary.path.join(&draft.folder).join(MAP_MANIFEST_FILE_NAME);
    let mut prepared = prepare_draft_writes(&temporary.path, &draft, None, None).unwrap();
    assert!(!prepared.writes.iter().any(|write| write.path == temporary.path.join(feathrise_map_content::MAP_CATALOG_ASSET_PATH)),
        "只有私有网格的水/植被补登记时，不得误写地图根目录入口");
    prepared.writes.retain(|write| write.path != manifest_path);
    for projection in &projections {
        prepared.writes.extend(prepare_draft_writes(&temporary.path, projection, None, None).unwrap().writes);
    }
    prepared.writes.push(PreparedWrite::json(manifest_path.clone(), &manifest));
    let blend_path = glb_path.with_extension("blend");
    assert!(prepared.writes.iter().any(|write| write.path == glb_path));
    assert!(!prepared.writes.iter().any(|write| write.path == blend_path), "未配置可选工具时不计划Blend写项");
    // GLB/JSON/R8/KTX2直接走完整生产保存事务，不再由测试手工剔除Blend阶段。
    commit_prepared_writes(prepared.writes).unwrap_or_else(|failure| panic!("高度域与WIP同源保存失败：{}", failure.message()));

    let reopened: MapManifestAsset = serde_json::from_slice(&std::fs::read(manifest_path).unwrap()).unwrap();
    feathrise_map_content::validate_terrain_objects(&reopened).unwrap();
    let object = reopened.terrains.as_ref().unwrap().iter().find(|object| object.id == id).unwrap();
    assert_eq!(object.terrain.glb_path, current.terrain.glb_path, "域修改不另换几何源引用");
    assert_eq!(object.terrain.base_height, -17.5);
    assert_eq!(object.terrain.height_scale, 40.0);
    let encoded = std::fs::read(&glb_path).unwrap();
    assert_ne!(encoded, baseline, "保存必须写入已雕刻WIP而不是旧平面");
    let geometry = feathrise_map_terrain::parse_dynamic_terrain_glb(&encoded).unwrap();
    assert_eq!(geometry.positions, mesh.positions);
    assert_eq!(geometry.indices, mesh.indices);
    let derived = feathrise_map_terrain::derive_terrain(geometry, spec).unwrap();
    assert_eq!(&derived.heights, draft.heights.as_ref().unwrap().as_ref());
    let surface: MapSurfaceAsset = serde_json::from_slice(
        &std::fs::read(temporary.path.join(object.surface_path.as_ref().unwrap())).unwrap()).unwrap();
    assert_eq!(serde_json::to_value(&surface).unwrap(), serde_json::to_value(draft.surface.as_ref().unwrap()).unwrap());
    assert_eq!(std::fs::read(temporary.path.join(&surface.weight_source_path)).unwrap().as_slice(), draft.surface_weights.as_ref().unwrap().as_slice());
    assert_eq!(std::fs::read(temporary.path.join(&surface.index_source_path)).unwrap().as_slice(), draft.surface_indices.as_ref().unwrap().as_slice());
    assert!(temporary.path.join(&surface.weight_texture_path).is_file());
    assert!(temporary.path.join(&surface.index_texture_path).is_file());
    let vegetation: MapVegetationAsset = serde_json::from_slice(
        &std::fs::read(temporary.path.join(object.vegetation_path.as_ref().unwrap())).unwrap()).unwrap();
    assert_eq!(serde_json::to_value(&vegetation).unwrap(), serde_json::to_value(draft.vegetation.as_ref().unwrap()).unwrap());
    for layer in &vegetation.layers {
        assert_eq!(std::fs::read(temporary.path.join(&layer.density_source_path)).unwrap().as_slice(), draft.vegetation_densities[&layer.id].as_slice());
        assert!(temporary.path.join(&layer.density_texture_path).is_file());
    }
    for layer in &vegetation.authoring.as_ref().unwrap().layers {
        assert_eq!(std::fs::read(temporary.path.join(&layer.density_source_path)).unwrap().as_slice(), draft.vegetation_author_densities[&layer.id].as_slice());
    }
    let water: MapWaterAsset = serde_json::from_slice(
        &std::fs::read(temporary.path.join(object.water_path.as_ref().unwrap())).unwrap()).unwrap();
    water.validate(&reopened.id).unwrap();
    assert_eq!(serde_json::to_value(&water).unwrap(), serde_json::to_value(draft.water.as_ref().unwrap()).unwrap());
    for layer in &water.layers {
        assert_eq!(std::fs::read(temporary.path.join(&layer.mask_source_path)).unwrap().as_slice(), draft.water_masks[&layer.id].as_slice());
        assert!(temporary.path.join(&layer.mask_texture_path).is_file());
    }
    let puddles = object.puddles.as_ref().unwrap();
    assert_eq!(std::fs::read(temporary.path.join(&puddles.source_path)).unwrap().as_slice(), draft.puddle_mask.as_ref().unwrap().as_slice());
    assert!(temporary.path.join(&puddles.texture_path).is_file());
    assert!(reopened.terrain.is_empty(), "保存仍只更新独立源，不能向地图容器生成旧主地形");
}
