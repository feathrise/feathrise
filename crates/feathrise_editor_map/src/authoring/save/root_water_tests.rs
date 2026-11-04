//! 空地图从真实新建按钮、刷水到事务保存/后台重读；根水与旧私有水必须始终各归自己的文件。
use super::super::*;
use crate::authoring::{EditorAuthoringSession, WaterBrushStroke,
    terrain_documents::{TerrainCanvas, TerrainDocumentIdentity}, water_load};
use bevy::prelude::*;
use feathrise_map_content::{MAP_WATER_FILE_NAME, MapManifestAsset, TerrainId};

fn painted_root() -> EditorMapDraft {
    let mut draft = crate::ui::water_layers::tests::empty_root_created_via_button_for_tests();
    draft.manifest.schema_version = MAP_SCHEMA_VERSION;
    draft.placements.schema_version = MAP_SCHEMA_VERSION;
    draft.portals.schema_version = MAP_SCHEMA_VERSION;
    draft.water.as_mut().unwrap().layers[0].water_level = 12.75;
    let layer = draft.water.as_ref().unwrap().layers[0].clone();
    crate::authoring::water_paint::paint_roundtrip_fixture(&mut draft, &layer);
    draft
}

fn reopen_water(root: &Path, source: &EditorMapDraft, manifest: MapManifestAsset, path: String) -> EditorMapDraft {
    let mut reopened = source.clone();
    reopened.manifest = manifest;
    reopened.water = None;
    reopened.water_masks.clear();
    reopened.water_dirty_layers.clear();
    reopened.water_asset_path = path;
    reopened.terrain_documents = Default::default();
    water_load::load_unregistered_clipmap_water(root, &mut reopened).unwrap();
    water_load::load_sources(root, &mut reopened).unwrap();
    reopened
}

#[test]
fn empty_map_button_paint_save_and_reopen_keeps_actual_json_path_water_level_bounds_and_r8() {
    let temporary = crate::authoring::save::tests::SaveTransactionTestDirectory::new();
    let mut draft = painted_root();
    draft.water_asset_path = format!("{}/custom-root.water.json", draft.folder);
    draft.water_catalog_registration_required = true;
    draft.water_registration_source_hash = Some("missing".into());
    let catalog_path = temporary.path.join(feathrise_map_content::MAP_CATALOG_ASSET_PATH);
    std::fs::create_dir_all(catalog_path.parent().unwrap()).unwrap();
    std::fs::write(&catalog_path, serde_json::to_vec(&serde_json::json!({
        "schema_version": MAP_SCHEMA_VERSION,
        "maps": [{ "id": draft.map_id, "display_name": "空图静水", "folder": draft.folder }]
    })).unwrap()).unwrap();
    let layer = draft.water.as_ref().unwrap().layers[0].clone();
    let expected = draft.water_masks[&layer.id].clone();
    assert!(expected.iter().any(|pixel| *pixel > 0));
    let guard = crate::authoring::save::domain_files::DraftSourceSaveGuard::prepare(&temporary.path, &draft).unwrap();
    let prepared = prepare_draft_writes(&temporary.path, &draft, None, None).unwrap();
    assert!(prepared.water_changed);
    commit_prepared_writes_checked(prepared.writes, move |_, path| guard.check(path))
        .unwrap_or_else(|error| panic!("空图静水事务失败：{}", error.message()));
    let catalog: feathrise_map_content::MapCatalogAsset = serde_json::from_slice(&std::fs::read(catalog_path).unwrap()).unwrap();
    assert_eq!(catalog.maps[0].water_path.as_deref(), Some(draft.water_asset_path.as_str()));
    let manifest: MapManifestAsset = serde_json::from_slice(&std::fs::read(temporary.path.join(&draft.folder).join(MAP_MANIFEST_FILE_NAME)).unwrap()).unwrap();
    assert!(manifest.terrain.is_empty() && manifest.terrains.as_ref().unwrap().is_empty());
    let reopened = reopen_water(&temporary.path, &draft, manifest, draft.water_asset_path.clone());
    let loaded = &reopened.water.as_ref().unwrap().layers[0];
    assert_eq!(loaded.id, layer.id);
    assert_eq!(loaded.water_level, 12.75);
    assert_eq!(loaded.canvas_bounds, Some([[-128.0; 2], [128.0; 2]]));
    assert_eq!(reopened.water_masks[&loaded.id], expected);
    assert!(temporary.path.join(&loaded.mask_texture_path).is_file());
    assert!(!temporary.path.join(&draft.folder).join(MAP_WATER_FILE_NAME).exists(), "自定义路径不另造默认根水JSON");
    assert!(!temporary.path.join(&draft.folder).join("terrain.glb").exists());
}

#[test]
fn root_and_private_water_share_one_save_transaction_and_reopen_without_source_mixing() {
    let temporary = crate::authoring::save::tests::SaveTransactionTestDirectory::new();
    let mut draft = painted_root();
    let root_path = draft.water_asset_path.clone();
    let root_layer = draft.water.as_ref().unwrap().layers[0].clone();
    let root_pixels = draft.water_masks[&root_layer.id].clone();
    let mut object = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121])).manifest.terrain_objects_for_edit()[0].clone();
    object.id = TerrainId::parse("private-water-save").unwrap();
    let folder = object.private_folder(&draft.map_id);
    object.terrain.glb_path = format!("{folder}/terrain.glb");
    object.water_path = Some(format!("{folder}/{MAP_WATER_FILE_NAME}"));
    object.surface_path = None;
    object.vegetation_path = None;
    let mut private = crate::authoring::water_asset_with_layer_for_tests(&draft.map_id, &folder);
    private.layers[0].water_level = -3.25;
    let private_layer = private.layers[0].clone();
    let private_pixels = Arc::new(vec![83; 512*512]);
    let canvas = TerrainCanvas { identity: Some(TerrainDocumentIdentity {
        definition: Arc::new(object.clone()), manifest: object.source_manifest(&draft.manifest), folder }),
        water: Some(private), water_asset_path: object.water_path.clone().unwrap(),
        water_masks: std::collections::BTreeMap::from([(private_layer.id.clone(), private_pixels.clone())]),
        water_dirty_layers: std::collections::BTreeSet::from([private_layer.id.clone()]), ..Default::default() };
    draft.manifest.terrains = Some(Arc::from([object.clone()]));
    draft.terrain_documents.values.insert(object.id.clone(), Arc::new(canvas));
    let mut selection = crate::ui::terrain_objects::TerrainObjectsEditorState::default();
    selection.selected = Some(object.id.clone());
    selection.selected_index = Some(0);
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(EditorAuthoringSession { draft: Some(draft), water_revision: 31, ..default() })
        .insert_resource(crate::resources::MapEditorState { open: true, ..default() })
        .insert_resource(selection)
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<WaterBrushStroke>();
    crate::authoring::terrain_documents::install(&mut app);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().terrain_documents.active.is_none() {
        app.update();
        assert!(std::time::Instant::now() < deadline, "实际画布focus应采用缓存私有源");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let mut draft = app.world_mut().remove_resource::<EditorAuthoringSession>().unwrap().draft.unwrap();
    assert_eq!(crate::authoring::terrain_documents::root_water::revision(&draft, 900), 31);
    let (manifest, projections, saved) = bevy::tasks::block_on(crate::authoring::save::terrain_documents::prepare(&draft)).unwrap();
    assert_eq!(saved, [object.id.clone()]);
    draft.manifest = manifest.unwrap();
    {
        let mut prepared = prepare_draft_writes(&temporary.path, &draft, None, None).unwrap();
        for projection in &projections { prepared.writes.extend(prepare_draft_writes(&temporary.path, projection, None, None).unwrap().writes); }
        commit_prepared_writes(prepared.writes).unwrap_or_else(|error| panic!("根水和私有水联合事务失败：{}", error.message()));
    } // 事务保存器借用冻结草稿，完整释放后才能实际切回根画布。
    let root = reopen_water(&temporary.path, &draft, draft.manifest.clone(), root_path.clone());
    let private = reopen_water(&temporary.path, &projections[0], object.source_manifest(&draft.manifest), object.water_path.clone().unwrap());
    assert_eq!(root.water.as_ref().unwrap().layers[0].water_level, 12.75);
    assert_eq!(root.water_masks[&root_layer.id], root_pixels);
    assert_eq!(private.water.as_ref().unwrap().layers[0].water_level, -3.25);
    assert_eq!(private.water_masks[&private_layer.id], private_pixels);
    assert!(crate::authoring::terrain_documents::root_water::focus(&mut draft));
    assert_eq!(draft.water_asset_path, root_path);
    assert_eq!(draft.water_masks[&root_layer.id], root_pixels);
    assert_eq!(draft.terrain_documents.values[&object.id].water_masks[&private_layer.id], private_pixels);
    assert!(!temporary.path.join(object.private_folder(&draft.map_id)).join(MAP_MANIFEST_FILE_NAME).exists());
}

#[test]
fn saved_root_water_stays_in_lists_after_selecting_empty_private_clipmap_and_can_be_selected_again() {
    use bevy::ecs::system::RunSystemOnce;
    use crate::{components::*, resources::*, ui::scene_context_menu::SceneObjectKey};
    let temporary = crate::authoring::save::tests::SaveTransactionTestDirectory::new();
    let mut draft = painted_root(); // 真新建按钮和生产画笔先形成有像素的独立根水。
    let root_path = draft.water_asset_path.clone();
    let layer = draft.water.as_ref().unwrap().layers[0].clone();
    let pixels = draft.water_masks[&layer.id].clone();
    let mut object = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121])).manifest.terrain_objects_for_edit()[0].clone();
    object.id = TerrainId::parse("saved-empty-private-clipmap").unwrap();
    object.terrain.kind = feathrise_map_content::MapTerrainKind::SparseClipmap;
    object.world_min = [-128.0; 2];
    object.world_max = [128.0; 2];
    object.terrain.glb_path.clear();
    object.terrain.resolution = [257; 2]; // 完整保存预检要求合法Clipmap尺寸与1米逻辑格距。
    object.terrain.collision_lods = feathrise_map_content::default_collision_lods_for_resolution(object.terrain.resolution);
    object.terrain.height_texture_path = None;
    object.surface_path = None;
    object.vegetation_path = None;
    let folder = object.private_folder(&draft.map_id);
    let spec = feathrise_map_content::SparseHeightSpec::new(object.world_min, object.world_max, 1.0,
        object.terrain.base_height, object.terrain.height_scale).unwrap();
    let mut height = feathrise_map_content::SparseHeightDocument::create_for_terrain(&temporary.path,
        draft.map_id.clone(), Some(object.id.clone()), spec).unwrap();
    height.save_dirty().unwrap(); // 复用新建网格的正式稀疏源写入，避免测试手造无效版本路径。
    object.terrain.height_path = Some(height.manifest().asset_path());
    object.water_path = Some(format!("{folder}/{MAP_WATER_FILE_NAME}"));
    draft.manifest.terrains = Some(Arc::from([object.clone()]));
    draft.terrain_documents.values.insert(object.id.clone(), Arc::new(TerrainCanvas {
        identity: Some(TerrainDocumentIdentity { definition: Arc::new(object.clone()),
            manifest: object.source_manifest(&draft.manifest), folder: folder.clone() }),
        water: Some(crate::authoring::default_water_asset(&draft.map_id)),
        water_asset_path: object.water_path.clone().unwrap(), structural_dirty: true, ..default()
    }));
    let (manifest, projections, _) = bevy::tasks::block_on(crate::authoring::save::terrain_documents::prepare(&draft)).unwrap();
    draft.manifest = manifest.unwrap();
    let mut prepared = prepare_draft_writes(&temporary.path, &draft, None, None).unwrap();
    for projection in &projections { prepared.writes.extend(prepare_draft_writes(&temporary.path, projection, None, None).unwrap().writes); }
    commit_prepared_writes(prepared.writes).unwrap_or_else(|error| panic!("根水和空私有文档事务失败：{}", error.message())); // 根水与空私有文档真实写入同一正式事务。
    let manifest = serde_json::from_slice(&std::fs::read(temporary.path.join(&draft.folder).join(MAP_MANIFEST_FILE_NAME)).unwrap()).unwrap();
    let mut reopened = reopen_water(&temporary.path, &draft, manifest, root_path.clone());
    let mut private = reopen_water(&temporary.path, &projections[0], object.source_manifest(&reopened.manifest), object.water_path.clone().unwrap());
    assert!(private.water.as_ref().unwrap().layers.is_empty());
    reopened.terrain_documents.values.insert(object.id.clone(), Arc::new(TerrainCanvas {
        identity: Some(TerrainDocumentIdentity { definition: Arc::new(object.clone()),
            manifest: object.source_manifest(&reopened.manifest), folder }),
        water: private.water.take(), water_masks: std::mem::take(&mut private.water_masks),
        water_asset_path: private.water_asset_path.clone(), ..default()
    }));
    let map = reopened.map_id.clone();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(EditorAuthoringSession { draft: Some(reopened), water_revision: 41, ..default() })
        .insert_resource(MapEditorState { open: true, selected_map_id: Some(map), ..default() })
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<WaterBrushStroke>()
        .init_resource::<crate::authoring::WaterNetworkEditState>()
        .init_resource::<crate::viewport::SelectedWaterViewportState>()
        .init_resource::<crate::viewport::LayerCandidateState>()
        .init_resource::<MapEditorNumericScrubMouseGuard>()
        .init_resource::<crate::ui::scene_layers::SceneLayerSelection>()
        .init_resource::<crate::ui::scene_context_menu::SceneContextMenuState>()
        .init_resource::<crate::ui::terrain_objects::TerrainObjectsEditorState>();
    crate::authoring::terrain_documents::install(&mut app);
    let title = app.world_mut().spawn((MapEditorWaterFeatureListTitle, Text::new(""))).id();
    let text = app.world_mut().spawn((MapEditorWaterFeatureRowText, Text::new(""), TextColor::default())).id();
    let row = app.world_mut().spawn((MapEditorWaterFeatureRow { slot: 0,
        kind: crate::types::WaterAuthoringFeatureKind::StillWater, feature_id: None, terrain: None },
        Node::default(), BackgroundColor::default(), feathrise_editor_ui::EditorButtonClick::default())).add_child(text).id();
    app.world_mut().spawn((MapEditorWaterFeatureList, Node::default()));
    // 连续两次走真实画布focus；分别验证水域列表点击和场景树点击都可把根水恢复为可写源。
    for from_tree in [false, true] {
        {
            let mut selection = app.world_mut().resource_mut::<crate::ui::terrain_objects::TerrainObjectsEditorState>();
            selection.selected = Some(object.id.clone());
            selection.selected_index = Some(0);
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            app.update();
            if app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().terrain_documents.active.is_some() { break; }
            assert!(std::time::Instant::now() < deadline, "真实空私有画布应完成采用");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().water.as_ref().unwrap().layers.is_empty());
        app.world_mut().run_system_once(crate::ui::water_feature_list::sync_water_feature_rows).unwrap();
        assert!(app.world().get::<Text>(title).unwrap().contains("1项"));
        let binding = app.world().get::<MapEditorWaterFeatureRow>(row).unwrap();
        assert_eq!(binding.feature_id.as_deref(), Some(layer.id.as_str()));
        assert!(binding.terrain.is_none());
        let targets = crate::ui::scene_layers::tests::water_snapshot(app.world().resource::<EditorAuthoringSession>());
        assert_eq!(targets.len(), 1);
        assert!(matches!(&targets[0].key, SceneObjectKey::Water { terrain: None, id, .. } if id == &layer.id));
        if from_tree {
            app.world_mut().resource_mut::<crate::ui::scene_layers::SceneLayerSelection>().0 = Some(targets[0].clone());
            app.world_mut().run_system_once(crate::ui::scene_context_menu::select_target_for_tests).unwrap();
        } else {
            app.world_mut().entity_mut(row).insert(feathrise_editor_ui::EditorButtonClick::triggered());
            app.world_mut().run_system_once(crate::ui::water_feature_list::handle_water_feature_row_clicks).unwrap();
            app.world_mut().entity_mut(row).insert(feathrise_editor_ui::EditorButtonClick::default());
        }
        app.update(); // 真实focus下一帧不能因残留网格选择把根水再次停放。
        let session = app.world().resource::<EditorAuthoringSession>();
        let draft = session.draft.as_ref().unwrap();
        assert!(draft.terrain_documents.active.is_none());
        assert_eq!(draft.water_asset_path, root_path);
        assert_eq!(draft.water.as_ref().unwrap().layers[0].water_level, layer.water_level);
        assert_eq!(draft.water_masks[&layer.id], pixels);
        assert!(draft.terrain_documents.values[&object.id].water.as_ref().unwrap().layers.is_empty());
        assert!(crate::authoring::water_canvas::writable(session, app.world().resource::<MapEditorState>()));
        assert!(session.undo.is_empty() && session.redo.is_empty(), "纯恢复和选择不新增历史");
        assert!(!app.world().resource::<MapEditorState>().dirty);
    }
}
