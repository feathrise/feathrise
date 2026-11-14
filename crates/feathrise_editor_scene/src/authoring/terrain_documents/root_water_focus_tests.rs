//! 根水与私有完整作者槽切换回归：无水画布可恢复，保存基线只应用到对应已保存缓存。
use super::{TerrainCanvas, TerrainDocumentIdentity, root_water};
use crate::authoring::{EditorSceneDraft, test_heightfield_draft, water_asset_with_layer_for_tests};
use feathrise_scene_content::{
    SCENE_SCHEMA_VERSION, SceneSurfaceLayer, SceneTerrainDescriptor, SceneVegetationAsset,
    SceneVegetationKind, SceneVegetationLayer, TerrainId,
};
use std::sync::Arc;

/// 活动槽只有根水；私有槽包含真实地表、植被和GLB作者数据，但没有water文档。
fn cached_private_without_water() -> (EditorSceneDraft, TerrainId, Arc<TerrainCanvas>) {
    let mut draft = test_heightfield_draft(Arc::new(vec![0; 121]));
    let mut object = draft.manifest.terrain_objects_for_edit()[0].clone();
    object.id = TerrainId::parse("cached-private").unwrap();
    let id = object.id.clone();
    let folder = object.private_folder(&draft.scene_id);
    object.terrain.glb_path = format!("{folder}/terrain.glb");
    object.surface_path = Some(format!("{folder}/surface.scene_surface.json"));
    object.vegetation_path = Some(format!("{folder}/vegetation.scene_vegetation.json"));
    object.water_path = None;
    draft.manifest.terrain = SceneTerrainDescriptor::empty();
    draft.manifest.terrains = Some(Arc::from([object.clone()]));

    let mut canvas = TerrainCanvas::snapshot(&draft);
    canvas.identity = Some(TerrainDocumentIdentity {
        manifest: object.source_manifest(&draft.manifest),
        folder: folder.clone(),
        definition: Arc::new(object),
    });
    let mut surface = crate::authoring::create::new_scene_surface_asset(&draft.scene_id, &folder);
    surface.resolution = [4, 4];
    surface.layers.push(SceneSurfaceLayer {
        id: "private-ground".into(),
        label: "私有地表".into(),
        preset_id: "ground".into(),
    });
    canvas.surface = Some(surface);
    canvas.surface_weights = Some(Arc::new([255, 0, 0, 0].repeat(16)));
    canvas.surface_indices = Some(Arc::new(vec![0; 16 * 16]));
    let mut vegetation = SceneVegetationAsset {
        model_surfaces: Vec::new().into(),
        schema_version: SCENE_SCHEMA_VERSION,
        scene_id: draft.scene_id.clone(),
        layers: vec![SceneVegetationLayer {
            id: "private-grass".into(),
            label: "私有草地".into(),
            kind: SceneVegetationKind::Blade,
            runtime_profile: Some("ordinary_grass".into()),
            runtime_layer_id: Some(1),
            prototype_id: None,
            asset_path: "grass.glb".into(),
            density_source_path: format!("{folder}/vegetation/private-grass.r8"),
            density_texture_path: format!("{folder}/vegetation/private-grass.ktx2"),
            resolution: [4, 4],
            spacing_meters: 1.0,
            scale_range: [1.0, 1.0],
            max_instances: 16,
            seed: 1,
        }],
        authoring: None,
    };
    crate::authoring::vegetation_defaults::ensure_vegetation_authoring(&mut vegetation, &folder);
    let author = vegetation.authoring.as_ref().unwrap().layers[0].id.clone();
    let density = Arc::new(vec![73; 16]);
    canvas.vegetation = Some(vegetation);
    canvas
        .vegetation_densities
        .insert("private-grass".into(), density.clone());
    canvas
        .vegetation_author_densities
        .insert(author.clone(), density);
    canvas
        .vegetation_dirty_layers
        .insert("private-grass".into());
    canvas.vegetation_author_dirty_layers.insert(author);
    canvas.structural_dirty = true;
    canvas.terrain_dirty = true;
    canvas.surface_dirty = true;
    canvas.terrain_source_sha256 = Some([11; 32]);
    canvas.terrain_overwrite_authorized_sha256 = Some([19; 32]);
    canvas.water_asset_path.clear();
    let cached = Arc::new(canvas);

    // 根水不得夹带私有画布的像素；用正式整体交换清空活动作者域后再放入根水。
    TerrainCanvas::default().swap(&mut draft);
    let mut water = water_asset_with_layer_for_tests(&draft.scene_id, &draft.folder);
    water.layers[0].id = "root-water".into();
    water.layers[0].resolution = [4, 4];
    water.layers[0].water_level = 7.0;
    draft.water = Some(water);
    draft.water_asset_path = format!("{}/water.scene_water.json", draft.folder);
    draft
        .water_masks
        .insert("root-water".into(), Arc::new(vec![17; 16]));
    draft.water_dirty_layers.insert("root-water".into());
    draft.terrain_documents.active_dirty = true;
    draft.terrain_documents.root_water_revision = 57;
    draft
        .terrain_documents
        .values
        .insert(id.clone(), cached.clone());
    draft.terrain_documents.dirty_stored.insert(id.clone());
    (draft, id, cached)
}

fn assert_private_pixels(draft: &EditorSceneDraft, cached: &TerrainCanvas) {
    assert!(draft.water.is_none() && draft.water_masks.is_empty());
    assert_eq!(draft.water_asset_path, "");
    assert!(Arc::ptr_eq(
        draft.terrain_mesh.as_ref().unwrap(),
        cached.terrain_mesh.as_ref().unwrap()
    ));
    assert!(Arc::ptr_eq(
        draft.heights.as_ref().unwrap(),
        cached.heights.as_ref().unwrap()
    ));
    assert!(Arc::ptr_eq(
        draft.surface_weights.as_ref().unwrap(),
        cached.surface_weights.as_ref().unwrap()
    ));
    assert!(Arc::ptr_eq(
        draft.surface_indices.as_ref().unwrap(),
        cached.surface_indices.as_ref().unwrap()
    ));
    assert_eq!(
        draft.surface.as_ref().unwrap().weight_source_path,
        cached.surface.as_ref().unwrap().weight_source_path
    );
    assert_eq!(
        draft.surface.as_ref().unwrap().layers,
        cached.surface.as_ref().unwrap().layers
    );
    assert_eq!(
        draft.vegetation.as_ref().unwrap().layers,
        cached.vegetation.as_ref().unwrap().layers
    );
    assert_eq!(
        draft.vegetation.as_ref().unwrap().authoring,
        cached.vegetation.as_ref().unwrap().authoring
    );
    for (id, pixels) in &cached.vegetation_densities {
        assert!(Arc::ptr_eq(&draft.vegetation_densities[id], pixels));
    }
    for (id, pixels) in &cached.vegetation_author_densities {
        assert!(Arc::ptr_eq(&draft.vegetation_author_densities[id], pixels));
    }
}

#[test]
fn cached_focus_restores_surface_and_vegetation_without_water_and_keeps_root_water_separate() {
    let (mut draft, id, cached) = cached_private_without_water();
    let root_pixels = draft.water_masks["root-water"].clone();
    let root_path = draft.water_asset_path.clone();
    assert!(root_water::validate_listed_focus(&draft, Some(&id)).is_err());
    root_water::validate_cached_focus(&draft, Some(&id)).unwrap();
    assert!(root_water::focus_cached(&mut draft, Some(&id)).unwrap());
    assert_private_pixels(&draft, &cached);
    let active = draft.terrain_documents.active.as_ref().unwrap();
    assert_eq!(
        active.definition.as_ref(),
        cached.identity.as_ref().unwrap().definition.as_ref()
    );
    assert_eq!(active.folder, cached.identity.as_ref().unwrap().folder);
    assert!(!draft.terrain_documents.values.contains_key(&id));
    assert!(draft.terrain_dirty && draft.surface_dirty && draft.terrain_documents.active_dirty);
    assert_eq!(
        draft.vegetation_dirty_layers,
        cached.vegetation_dirty_layers
    );
    assert_eq!(
        draft.vegetation_author_dirty_layers,
        cached.vegetation_author_dirty_layers
    );
    assert!(!root_water::focus_cached(&mut draft, Some(&id)).unwrap());
    let root = root_water::source(&draft);
    assert_eq!(root.water.unwrap().layers[0].water_level, 7.0);
    assert_eq!(root.asset_path, root_path);
    assert!(Arc::ptr_eq(&root.masks["root-water"], &root_pixels));
    assert!(root.dirty_layers.contains("root-water"));
    assert_eq!(root_water::revision(&draft, 99), 57);

    // 往返恢复必须继续使用原Arc与原私有路径，不能因为无水而误留在场景根槽。
    assert!(root_water::focus_cached(&mut draft, None).unwrap());
    assert!(draft.terrain_documents.active.is_none());
    assert!(draft.surface.is_none() && draft.vegetation.is_none() && draft.heights.is_none());
    assert!(Arc::ptr_eq(&draft.water_masks["root-water"], &root_pixels));
    assert_eq!(draft.water_asset_path, root_path);
    assert!(draft.terrain_documents.dirty_stored.contains(&id));
    assert!(root_water::focus_cached(&mut draft, Some(&id)).unwrap());
    assert_private_pixels(&draft, &cached);
}

#[test]
fn saved_cached_focus_adopts_saved_digest_and_clears_overwrite_authorization() {
    let (mut draft, id, cached) = cached_private_without_water();
    draft.terrain_documents.saved.insert(id.clone());
    draft
        .terrain_documents
        .saved_digests
        .insert(id.clone(), [41; 32]);
    assert!(root_water::focus_cached(&mut draft, Some(&id)).unwrap());
    assert_private_pixels(&draft, &cached);
    assert_eq!(draft.terrain_source_sha256, Some([41; 32]));
    assert_eq!(draft.terrain_overwrite_authorized_sha256, None);
    assert!(!TerrainCanvas::snapshot(&draft).dirty());
    assert!(!draft.terrain_documents.saved.contains(&id));
    assert!(!draft.terrain_documents.dirty_stored.contains(&id));
    assert!(draft.terrain_documents.root_water.as_ref().unwrap().dirty());
}

#[test]
fn unsaved_cached_focus_keeps_its_own_digest_despite_old_saved_digest_for_same_id() {
    let (mut draft, id, cached) = cached_private_without_water();
    draft
        .terrain_documents
        .saved_digests
        .insert(id.clone(), [41; 32]);
    assert!(!draft.terrain_documents.saved.contains(&id));
    assert!(root_water::focus_cached(&mut draft, Some(&id)).unwrap());
    assert_private_pixels(&draft, &cached);
    assert_eq!(draft.terrain_source_sha256, Some([11; 32]));
    assert_eq!(draft.terrain_overwrite_authorized_sha256, Some([19; 32]));
    assert!(draft.terrain_dirty && draft.surface_dirty && draft.terrain_documents.active_dirty);
    assert_eq!(
        draft.vegetation_dirty_layers,
        cached.vegetation_dirty_layers
    );
    assert_eq!(
        draft.vegetation_author_dirty_layers,
        cached.vegetation_author_dirty_layers
    );
}

#[test]
fn rejected_cached_focus_preserves_root_slot_cached_pixels_and_pending_save_markers() {
    for reason in ["loading", "changed-definition", "vegetation-only"] {
        let (mut draft, id, _) = cached_private_without_water();
        match reason {
            "loading" => draft.terrain_documents.loading = true,
            "changed-definition" => {
                // 不改ID或源路径，仍须拒绝完整定义已经更新的旧作者槽。
                Arc::make_mut(draft.manifest.terrains.as_mut().unwrap())[0]
                    .display_name
                    .push_str("（已更新）");
            }
            "vegetation-only" => {
                Arc::make_mut(draft.terrain_documents.values.get_mut(&id).unwrap())
                    .vegetation_only = true
            }
            _ => unreachable!(),
        }
        draft.terrain_documents.saved.insert(id.clone());
        draft
            .terrain_documents
            .saved_digests
            .insert(id.clone(), [41; 32]);
        let cached = draft.terrain_documents.values[&id].clone();
        let root_pixels = draft.water_masks["root-water"].clone();
        let root_path = draft.water_asset_path.clone();
        let error = root_water::validate_cached_focus(&draft, Some(&id)).unwrap_err();
        assert_eq!(
            root_water::focus_cached(&mut draft, Some(&id)).unwrap_err(),
            error,
            "{reason}"
        );
        assert!(draft.terrain_documents.active.is_none(), "{reason}");
        assert!(draft.terrain_documents.root_water.is_none(), "{reason}");
        assert_eq!(draft.terrain_documents.values.len(), 1, "{reason}");
        assert!(
            Arc::ptr_eq(&draft.terrain_documents.values[&id], &cached),
            "{reason}"
        );
        assert!(
            Arc::ptr_eq(&draft.water_masks["root-water"], &root_pixels),
            "{reason}"
        );
        assert_eq!(draft.water_asset_path, root_path, "{reason}");
        assert_eq!(
            draft.water.as_ref().unwrap().layers[0].water_level,
            7.0,
            "{reason}"
        );
        assert!(draft.water_dirty_layers.contains("root-water"), "{reason}");
        assert!(draft.terrain_documents.active_dirty, "{reason}");
        assert!(draft.terrain_documents.saved.contains(&id), "{reason}");
        assert!(
            draft.terrain_documents.dirty_stored.contains(&id),
            "{reason}"
        );
        assert_eq!(
            draft.terrain_documents.saved_digests[&id], [41; 32],
            "{reason}"
        );
        assert_eq!(cached.terrain_source_sha256, Some([11; 32]), "{reason}");
        assert_eq!(
            cached.terrain_overwrite_authorized_sha256,
            Some([19; 32]),
            "{reason}"
        );
    }
}
