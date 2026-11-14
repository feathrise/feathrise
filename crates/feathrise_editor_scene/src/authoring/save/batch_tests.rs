//! 保存批次的摆放、水体预算与地表语义写入回归。

use bevy::math::{Rect, Vec2}; // 摆放公共变换校验需要明确测试场景XZ范围。
use feathrise_scene_content::{
    MAX_SCENE_PLACEMENT_WATER_SURFACES_PER_SCENE, ScenePlacementRecord, ScenePlacementWaterKind,
    ScenePlacementWaterSurface, SceneSurfaceLayer, SCENE_PLACEMENTS_FILE_NAME,
}; // 使用正式摆放、水体与地表格式验证保存前失败门和语义写入决定。
use sha2::{Digest, Sha256}; // 摆放 GLB 并发保存回归核对真实源摘要。

use super::*; // 直接测试不访问文件系统的保存批次纯预检。

#[path = "root_water_tests.rs"]
mod root_water_tests;

#[test]
fn ordinary_non_water_outputs_reject_preserved_document_alias_without_writing_or_staging() {
    let temporary = super::super::tests::SaveTransactionTestDirectory::new();
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    draft.manifest.schema_version = SCENE_SCHEMA_VERSION;
    draft.placements.schema_version = SCENE_SCHEMA_VERSION;
    draft.portals.schema_version = SCENE_SCHEMA_VERSION;
    draft.manifest.terrain = feathrise_scene_content::SceneTerrainDescriptor::empty();
    draft.terrain_mesh = None;
    draft.heights = None;
    draft.surface = None;
    draft.vegetation = None;
    draft.water = None;
    draft.boundaries = None;
    let folder = temporary.path.join(&draft.folder);
    std::fs::create_dir_all(&folder).unwrap();
    let files = [
        SCENE_MANIFEST_FILE_NAME,
        SCENE_PLACEMENTS_FILE_NAME,
        SCENE_PORTALS_FILE_NAME,
    ];
    let original = br#"{ "private_game_schema": 998, "body": "preserve raw bytes" }"#;
    for file in files {
        std::fs::write(folder.join(file), original).unwrap();
    }
    for (index, file) in files.into_iter().enumerate() {
        let mut preserved = format!("{}/{file}", draft.folder);
        if index == 1 {
            preserved = preserved.to_ascii_uppercase();
        } // 大小写别名同样不得覆盖原正文。
        draft.preserved_asset_paths = Arc::from([preserved]);
        let error = prepare_draft_writes(&temporary.path, &draft, None)
            .err()
            .expect("普通非水目标必须拒绝冒用保留文档路径");
        assert!(error.contains("保留的外部作者文档路径冲突"), "{error}");
        for source in files {
            assert_eq!(std::fs::read(folder.join(source)).unwrap(), original);
        }
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), files.len()); // 失败发生在首个stage/backup创建前。
    }
}

#[test]
fn cross_mesh_grass_group_and_every_local_canvas_save_and_reopen_together() {
    let temporary=super::super::tests::SaveTransactionTestDirectory::new();
    let mut app=bevy::prelude::App::new();app.add_plugins(bevy::prelude::MinimalPlugins);
    let mut draft=crate::authoring::vegetation_paint_scope::painted_draft();
    draft.manifest.schema_version=SCENE_SCHEMA_VERSION;draft.placements.schema_version=SCENE_SCHEMA_VERSION;draft.portals.schema_version=SCENE_SCHEMA_VERSION;
    let (manifest,projections,saved)=bevy::tasks::block_on(super::super::terrain_documents::prepare(&draft)).unwrap();
    assert_eq!(saved.len(),2);assert_eq!(projections.len(),2);
    let manifest=manifest.unwrap();let manifest_path=temporary.path.join(&draft.folder).join(SCENE_MANIFEST_FILE_NAME);
    let mut prepared=prepare_draft_writes(&temporary.path,&draft, None).unwrap();
    prepared.writes.retain(|write|write.path!=manifest_path);
    for projection in &projections {prepared.writes.extend(prepare_draft_writes(&temporary.path,projection, None).unwrap().writes);}
    prepared.writes.push(PreparedWrite::json(manifest_path.clone(),&manifest));
    commit_prepared_writes(prepared.writes).unwrap_or_else(|e|panic!("跨网格联合保存失败：{}",e.message()));
    let owner:feathrise_scene_content::SceneManifestAsset=serde_json::from_slice(&std::fs::read(manifest_path).unwrap()).unwrap();
    assert_eq!(owner.vegetation_paint_groups[0].members.len(),2);
    for object in owner.terrains.as_ref().unwrap().iter() {
        let path=object.vegetation_path.as_ref().expect("每个实际落笔网格都要登记自己的植被文档");
        let vegetation:feathrise_scene_content::SceneVegetationAsset=serde_json::from_slice(&std::fs::read(temporary.path.join(path)).unwrap()).unwrap();
        let member=&owner.vegetation_paint_groups[0].members[&object.id];
        let layer=vegetation.authoring.as_ref().unwrap().layers.iter().find(|l|&l.id==member).unwrap();
        let pixels=std::fs::read(temporary.path.join(&layer.density_source_path)).unwrap();
        assert!(pixels.iter().any(|v|*v>0));assert!(layer.density_source_path.starts_with(&object.private_folder(&owner.id)));
        let output=&vegetation.layers[0];assert!(temporary.path.join(&output.density_texture_path).exists());
        assert_eq!(output.seed,12345);assert!(!temporary.path.join(object.private_folder(&owner.id)).join(SCENE_MANIFEST_FILE_NAME).exists());
    }
}

#[test]
fn blank_scene_save_and_reopen_never_writes_a_terrain_or_implicit_canvas() {
    let temporary = super::super::tests::SaveTransactionTestDirectory::new();
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    draft.manifest.schema_version = SCENE_SCHEMA_VERSION;
    draft.placements.schema_version = SCENE_SCHEMA_VERSION;
    draft.portals.schema_version = SCENE_SCHEMA_VERSION;
    draft.manifest.terrain = feathrise_scene_content::SceneTerrainDescriptor::empty();
    draft.manifest.terrains = Some(Arc::from([]));
    draft.terrain_mesh = None; draft.heights = None; draft.surface = None;
    draft.vegetation = Some(feathrise_scene_content::SceneVegetationAsset {
        model_surfaces: Vec::new().into(),
        schema_version: SCENE_SCHEMA_VERSION,
        scene_id: draft.scene_id.clone(),
        layers: Vec::new(),
        authoring: Some(crate::authoring::vegetation_defaults::empty_vegetation_authoring()),
    }); // 新建空图实际带有空植被描述，不能用None夹具漏掉容器占位状态。
    draft.water = None; draft.boundaries = None;
    draft.manifest.notes = "空图仅修改备注".into();
    let vegetation_path = temporary.path.join(&draft.folder).join(SCENE_VEGETATION_FILE_NAME);
    let vegetation_bytes = serde_json::to_vec(draft.vegetation.as_ref().unwrap()).unwrap();
    std::fs::create_dir_all(vegetation_path.parent().unwrap()).unwrap();
    std::fs::write(&vegetation_path, &vegetation_bytes).unwrap();
    let prepared = prepare_draft_writes(&temporary.path, &draft, None).unwrap();
    commit_prepared_writes(prepared.writes).unwrap_or_else(|e| panic!("空白场景保存失败：{}", e.message()));
    let folder = temporary.path.join(&draft.folder);
    let saved: feathrise_scene_content::SceneManifestAsset = serde_json::from_slice(&std::fs::read(folder.join(SCENE_MANIFEST_FILE_NAME)).unwrap()).unwrap();
    assert_eq!(saved.terrain_count(), 0);
    assert!(saved.terrains.is_some());
    assert_eq!(saved.notes, "空图仅修改备注");
    assert_eq!(std::fs::read(vegetation_path).unwrap(), vegetation_bytes, "保存容器不能改写占位植被文件或制造密度输出");
    assert!(!folder.join("vegetation").exists());
    for name in ["terrain.glb", SCENE_SURFACE_FILE_NAME, feathrise_scene_content::SCENE_WATER_FILE_NAME, SCENE_BOUNDARIES_FILE_NAME] {
        assert!(!folder.join(name).exists(), "空白场景不应隐式创建{name}");
    }
}

#[test]
fn object_clipmap_save_reopens_independent_explicit_lods() {
    use feathrise_scene_content::{SceneManifestAsset, SceneTerrainDescriptor, SceneTerrainKind, SparseHeightSpec,
        default_collision_lods_for_resolution, default_display_lods_for_spec};
    let mut app = bevy::prelude::App::new();
    app.add_plugins(bevy::prelude::MinimalPlugins);
    let temporary = super::super::tests::SaveTransactionTestDirectory::new();
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    draft.manifest.schema_version = SCENE_SCHEMA_VERSION;
    draft.placements.schema_version = SCENE_SCHEMA_VERSION;
    draft.portals.schema_version = SCENE_SCHEMA_VERSION;
    let mut object = draft.manifest.terrain_objects_for_edit()[0].clone();
    object.world_min = [-512.0; 2];
    object.world_max = [512.0; 2];
    object.terrain.kind = SceneTerrainKind::SparseClipmap;
    object.terrain.glb_path.clear();
    object.terrain.resolution = [4097; 2];
    object.terrain.height_path = Some(format!("scenes/{}/terrain/height_tiles.scene_height_manifest.json", draft.scene_id));
    object.terrain.collision_lods = default_collision_lods_for_resolution(object.terrain.resolution);
    object.terrain.collision_lods[0].max_distance_meters = 24.0;
    let spec = SparseHeightSpec::new(object.world_min, object.world_max, 0.25,
        object.terrain.base_height, object.terrain.height_scale).unwrap();
    let mut rows = default_display_lods_for_spec(&spec).unwrap();
    rows[0].outer_half_extent_meters = Some(20.0);
    object.terrain.display_lods = Some(rows);
    // 保存走正式画布投影和跨文件事务，所有文件均在隔离临时目录。
    draft.manifest.terrain = SceneTerrainDescriptor::empty();
    draft.manifest.terrains = Some(Arc::from([object.clone()]));
    draft.terrain_mesh = None;
    draft.heights = None;
    draft.surface = None;
    draft.vegetation = None;
    draft.water = None;
    draft.boundaries = None;
    let (manifest, projections, saved) = bevy::tasks::block_on(super::super::terrain_documents::prepare(&draft)).unwrap();
    assert!(projections.is_empty());
    assert!(saved.is_empty());
    draft.manifest = manifest.unwrap();
    let prepared = prepare_draft_writes(&temporary.path, &draft, None).unwrap();
    commit_prepared_writes(prepared.writes)
        .unwrap_or_else(|error| panic!("网格LOD保存失败：{}", error.message()));
    let path = temporary.path.join(&draft.folder).join(SCENE_MANIFEST_FILE_NAME);
    let reopened: SceneManifestAsset = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert!(reopened.terrain.is_empty());
    assert_eq!(reopened.terrains.as_deref(), Some(std::slice::from_ref(&object)));
    assert!(!temporary.path.join(&draft.folder).join("terrain.glb").exists());
}

#[test]
fn added_clean_glb_canvas_saves_object_specs_without_rewriting_the_source_or_root_terrain() {
    use crate::authoring::terrain_documents::TerrainDocumentIdentity;
    use feathrise_scene_content::{SceneTerrainDescriptor, TerrainId};
    use feathrise_terrain::{TerrainSampleSpec, build_flat_terrain_glb, parse_dynamic_terrain_glb};

    let temporary = super::super::tests::SaveTransactionTestDirectory::new();
    let mut app = bevy::prelude::App::new();
    app.add_plugins(bevy::prelude::MinimalPlugins); // 正式画布保存准备使用同一个后台计算池。
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    draft.manifest.schema_version = SCENE_SCHEMA_VERSION;
    draft.placements.schema_version = SCENE_SCHEMA_VERSION;
    draft.portals.schema_version = SCENE_SCHEMA_VERSION;
    let mut object = draft.manifest.terrain_objects_for_edit()[0].clone();
    object.id = TerrainId::parse("added-glb").unwrap();
    object.display_name = "新建GLB地形".into();
    object.transform.translation = [5.0, 0.0, 5.0];
    object.world_min = [-3.0, -2.0];
    object.world_max = [3.0, 2.0];
    object.terrain.resolution = [5, 3];
    object.terrain.base_height = -2.0;
    object.terrain.height_scale = 8.0;
    let folder = object.private_folder(&draft.scene_id);
    object.terrain.glb_path = format!("{folder}/terrain.glb");
    object.surface_path = None;
    object.vegetation_path = None;
    object.water_path = None;
    let spec = TerrainSampleSpec {
        resolution: object.terrain.resolution,
        world_min: object.world_min,
        world_max: object.world_max,
        base_height: object.terrain.base_height,
        height_scale: object.terrain.height_scale,
    };
    let source_bytes = build_flat_terrain_glb(spec, 1.0).unwrap();
    let mesh = parse_dynamic_terrain_glb(&source_bytes).unwrap();
    let derived = feathrise_terrain::derive_terrain(mesh.clone(), spec).unwrap();
    let source_path = temporary.path.join(&object.terrain.glb_path);
    std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    std::fs::write(&source_path, &source_bytes).unwrap();

    // 添加后的场景仍是空地形容器；合法采样规格只属于所选对象的源画布。
    draft.manifest.terrain = SceneTerrainDescriptor::empty();
    draft.manifest.terrains = Some(Arc::from([object.clone()]));
    draft.terrain_documents.active = Some(TerrainDocumentIdentity {
        manifest: object.source_manifest(&draft.manifest),
        definition: Arc::new(object.clone()),
        folder,
    });
    draft.terrain_editing_index = Some(Arc::new(
        feathrise_terrain::build_terrain_editing_index(&mesh).unwrap(),
    ));
    draft.terrain_mesh = Some(Arc::new(mesh));
    draft.heights = Some(Arc::new(derived.heights));
    draft.terrain_source_sha256 = Some(Sha256::digest(&source_bytes).into());
    assert_eq!(draft.manifest.terrain.resolution, [0, 0]);
    assert_eq!(draft.terrain_manifest().terrain.resolution, [5, 3]);

    let (manifest, projections, saved) =
        bevy::tasks::block_on(super::super::terrain_documents::prepare(&draft)).unwrap();
    assert!(projections.is_empty(), "载入干净网格不产生改写作者源的投影");
    assert!(saved.is_empty());
    draft.manifest = manifest.unwrap();
    let prepared = prepare_draft_writes(&temporary.path, &draft, None).unwrap();
    assert!(prepared.writes.iter().all(|write| write.path != source_path));
    commit_prepared_writes(prepared.writes)
        .unwrap_or_else(|failure| panic!("新增GLB对象保存失败：{}", failure.message()));
    let scene_folder = temporary.path.join(&draft.folder);
    let reopened: feathrise_scene_content::SceneManifestAsset = serde_json::from_slice(
        &std::fs::read(scene_folder.join(SCENE_MANIFEST_FILE_NAME)).unwrap(),
    )
    .unwrap();
    assert_eq!(reopened.terrain, SceneTerrainDescriptor::empty());
    assert_eq!(reopened.terrains.as_deref(), Some(std::slice::from_ref(&object)));
    assert_eq!(std::fs::read(&source_path).unwrap(), source_bytes);
    assert_eq!(
        parse_dynamic_terrain_glb(&std::fs::read(source_path).unwrap())
            .unwrap()
            .positions
            .len(),
        15,
    );
    for name in ["terrain.glb", "terrain.blend", SCENE_SURFACE_FILE_NAME, SCENE_VEGETATION_FILE_NAME] {
        assert!(!scene_folder.join(name).exists(), "容器不得制造{name}");
    }
}

fn object_canvas_fixture(owner: &feathrise_scene_content::SceneManifestAsset, name: &str, potential: u8)
    -> (feathrise_scene_content::SceneTerrainObject, crate::authoring::terrain_documents::TerrainCanvas) {
    use crate::authoring::terrain_documents::{TerrainCanvas, TerrainDocumentIdentity};
    let mut object = owner.terrain_objects_for_edit()[0].clone();
    object.id = feathrise_scene_content::TerrainId::parse(name).unwrap();
    let folder = object.private_folder(&owner.id);
    object.terrain.glb_path = format!("{folder}/terrain.glb");
    object.surface_path = Some(format!("{folder}/{SCENE_SURFACE_FILE_NAME}"));
    object.puddles = Some(feathrise_scene_content::ScenePuddleDescriptor { source_path: format!("{folder}/puddles/potential.r8"),
        texture_path: format!("{folder}/puddles/potential.ktx2"), resolution: [4,4] });
    let mut surface = crate::authoring::create::new_scene_surface_asset(&owner.id, &folder);
    surface.resolution = [4,4];
    let canvas = TerrainCanvas {
        identity: Some(TerrainDocumentIdentity { definition: Arc::new(object.clone()),
            manifest: object.source_manifest(owner), folder }),
        surface: Some(surface), surface_indices: Some(Arc::new(vec![0;16*16])),
        surface_weights: Some(Arc::new(vec![0;16*4])), surface_dirty: true,
        puddle_mask: Some(Arc::new(vec![potential;16])), puddles_dirty: true, ..Default::default()
    };
    (object,canvas)
}

#[test]
fn object_water_receipt_skips_unchanged_json_but_tracks_masks_parameters_and_last_layer_removal() {
    let temporary = super::super::tests::SaveTransactionTestDirectory::new();
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    draft.manifest.schema_version = SCENE_SCHEMA_VERSION;
    draft.placements.schema_version = SCENE_SCHEMA_VERSION;
    draft.portals.schema_version = SCENE_SCHEMA_VERSION;
    let (mut object, mut canvas) = object_canvas_fixture(&draft.manifest, "water-receipt", 7);
    let folder = object.private_folder(&draft.scene_id);
    let water_path = format!("{folder}/water.scene_water.json");
    object.water_path = Some(water_path.clone());
    canvas.identity.as_mut().unwrap().definition = Arc::new(object.clone());
    canvas.identity.as_mut().unwrap().manifest = object.source_manifest(&draft.manifest);
    let mut water = crate::authoring::water_defaults::water_asset_with_layer_for_tests(&draft.scene_id, &folder);
    water.layers[0].resolution = [4, 4];
    let raw = temporary.path.join(&water.layers[0].mask_source_path);
    let gpu = temporary.path.join(&water.layers[0].mask_texture_path);
    canvas.water_asset_path = water_path.clone();
    canvas.water = Some(water.clone());
    canvas.swap(&mut draft);
    draft.folder = folder;
    draft.manifest = draft.terrain_documents.active.as_ref().unwrap().manifest.clone();
    draft.terrain_documents.detached = true; // 使用正式独立画布投影的同一输入约束，所有文件均在隔离目录。
    let json = temporary.path.join(&water_path);
    std::fs::create_dir_all(json.parent().unwrap()).unwrap();
    std::fs::write(&json, serde_json::to_vec(&water).unwrap()).unwrap();
    {
        let prepared = prepare_draft_writes(&temporary.path, &draft, None).unwrap();
        assert!(!prepared.water_changed);
        assert!(prepared.writes.iter().all(|write| write.path != json),
            "仅地表/积水脏不能重发语义相同的独立水JSON或建立水交接");
    }
    draft.water_masks.insert("lake".into(), Arc::new(vec![127; 16]));
    draft.water_dirty_layers.insert("lake".into());
    {
        let prepared = prepare_draft_writes(&temporary.path, &draft, None).unwrap();
        assert!(prepared.water_changed);
        assert!(prepared.writes.iter().any(|write| write.path == raw));
        assert!(prepared.writes.iter().any(|write| write.path == gpu));
        commit_prepared_writes(prepared.writes).unwrap_or_else(|failure| panic!("独立水事务失败：{}", failure.message()));
    }
    assert_eq!(std::fs::read(&raw).unwrap(), vec![127; 16]);
    assert!(gpu.exists(), "raw和GPU派生必须仍走同一个完整事务");
    draft.water_dirty_layers.clear();
    draft.water.as_mut().unwrap().layers[0].water_level = 3.0;
    {
        let prepared = prepare_draft_writes(&temporary.path, &draft, None).unwrap();
        assert!(prepared.water_changed, "只有水参数变化也必须交接正式JSON");
        assert!(prepared.writes.iter().any(|write| write.path == json));
        assert!(prepared.writes.iter().all(|write| write.path != raw && write.path != gpu));
    }
    draft.water.as_mut().unwrap().layers.clear();
    {
        let prepared = prepare_draft_writes(&temporary.path, &draft, None).unwrap();
        assert!(prepared.water_changed, "删除最后一层不能被无脏R8误判为水无变化");
        commit_prepared_writes(prepared.writes).unwrap_or_else(|failure| panic!("清空独立水事务失败：{}", failure.message()));
    }
    let reopened: feathrise_scene_content::SceneWaterAsset = serde_json::from_slice(&std::fs::read(json).unwrap()).unwrap();
    assert!(reopened.layers.is_empty());
}

#[test]
fn two_dirty_object_canvases_save_and_reopen_with_separate_files() {
    let temporary = super::super::tests::SaveTransactionTestDirectory::new();
    let mut app = bevy::prelude::App::new(); app.add_plugins(bevy::prelude::MinimalPlugins);
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0;121]));
    draft.manifest.schema_version = SCENE_SCHEMA_VERSION;
    draft.placements.schema_version = SCENE_SCHEMA_VERSION; draft.portals.schema_version = SCENE_SCHEMA_VERSION;
    let (a, mut active) = object_canvas_fixture(&draft.manifest, "canvas-a",7);
    let (b, parked) = object_canvas_fixture(&draft.manifest, "canvas-b",11);
    draft.manifest.terrain = feathrise_scene_content::SceneTerrainDescriptor::empty();
    draft.manifest.terrains = Some(Arc::from([a.clone(), b.clone()]));
    draft.terrain_mesh = None; draft.heights = None; draft.water = None; draft.vegetation = None; draft.boundaries = None;
    active.swap(&mut draft);
    draft.terrain_documents.values.insert(b.id.clone(), Arc::new(parked));
    let (manifest, projections, saved) = bevy::tasks::block_on(super::super::terrain_documents::prepare(&draft)).unwrap();
    assert_eq!(saved.len(),2); assert_eq!(projections.len(),2);
    let manifest = manifest.unwrap();
    let mut prepared = prepare_draft_writes(&temporary.path,&draft, None).unwrap();
    let manifest_path = temporary.path.join(&draft.folder).join(SCENE_MANIFEST_FILE_NAME);
    prepared.writes.retain(|write| write.path != manifest_path);
    for projection in &projections { prepared.writes.extend(prepare_draft_writes(&temporary.path,projection, None).unwrap().writes); }
    prepared.writes.push(PreparedWrite::json(manifest_path.clone(),&manifest));
    commit_prepared_writes(prepared.writes).unwrap_or_else(|failure| panic!("多地形画布保存失败：{}", failure.message()));
    let reopened: feathrise_scene_content::SceneManifestAsset = serde_json::from_slice(&std::fs::read(manifest_path).unwrap()).unwrap();
    assert_eq!(reopened.terrain_count(),2);
    for (object, expected) in [(a,7u8),(b,11u8)] {
        let folder = temporary.path.join(object.private_folder(&draft.scene_id));
        let surface: SceneSurfaceAsset = serde_json::from_slice(&std::fs::read(folder.join(SCENE_SURFACE_FILE_NAME)).unwrap()).unwrap();
        assert_eq!(surface.resolution,[4,4]);
        assert_eq!(std::fs::read(temporary.path.join(object.puddles.unwrap().source_path)).unwrap(),vec![expected;16]);
        assert!(!folder.join(SCENE_MANIFEST_FILE_NAME).exists());
        assert!(!folder.join(SCENE_PLACEMENTS_FILE_NAME).exists());
        assert!(!folder.join(SCENE_PORTALS_FILE_NAME).exists());
    }
    assert!(!temporary.path.join(&draft.folder).join(SCENE_SURFACE_FILE_NAME).exists());
}

#[test]
fn object_canvas_cannot_write_another_objects_pixel_source() {
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0;121]));
    let (_,canvas) = object_canvas_fixture(&draft.manifest,"canvas-a",7);
    let mut projection = canvas;
    let folder = projection.identity.as_ref().unwrap().folder.clone();
    projection.surface.as_mut().unwrap().weight_source_path = format!("scenes/{}/terrains/canvas-b/surface/weights.rgba8",draft.scene_id);
    assert!(projection.validate_paths(Path::new("assets"),&folder).is_err());
    draft.terrain_documents.saved.clear();
}

fn placement_document(scene_id: &SceneId) -> ScenePlacementsAsset {
    ScenePlacementsAsset {
        schema_version: SCENE_SCHEMA_VERSION, // 使用当前正式格式版本。
        scene_id: scene_id.clone(),             // 文档身份必须与保存目标场景一致。
        placements: vec![ScenePlacementRecord {
            mesh_lod: None,
            terrain_binding: None,
            editor: Default::default(),
            terrain_patch: None,
            id: "placement-water".to_owned(), // 构造唯一稳定摆放身份。
            asset_path: "gltf/water.glb".to_owned(), // 测试不实际加载GLB。
            asset_target: Default::default(), // 水体预检必须基于Scene0层级。
            building_preset_id: None,         // 水体保存夹具不引用建筑预设。
            courtyard: None,                  // 独立摆放记录不属于任何院落副本。
            translation: [0.0, 0.0, 0.0],     // 使用有限默认变换。
            height_mode: feathrise_scene_content::ScenePlacementHeightMode::FixedHeight, // 保存批次夹具保持确定世界Y。
            terrain_offset: 0.0,    // 固定高度不携带离地偏移。
            yaw_degrees: 0.0,       // 使用有限默认旋转。
            scale: [1.0, 1.0, 1.0], // 使用非退化默认缩放。
            water_surfaces: vec![ScenePlacementWaterSurface {
                mesh_index: 0,                           // 索引只作正式诊断字段。
                node_path: vec!["水面对象".to_owned()],  // 稳定节点路径必须非空。
                mesh_name: "水面网格".to_owned(),        // 稳定网格名称必须非空。
                kind: ScenePlacementWaterKind::Horizontal, // 使用无需额外斜面输入的合法语义。
                flow_speed: 1.0,                         // 使用有限流速。
                height_offset: 0.0,                      // 使用有限世界Y偏移。
            }],
        }],
    }
}

fn surface_document(scene_id: &SceneId) -> SceneSurfaceAsset {
    SceneSurfaceAsset {
        schema_version: SCENE_SURFACE_SCHEMA_VERSION, // 使用当前索引式地表格式验证语义比较。
        scene_id: scene_id.clone(),                     // 快照必须保持本次场景身份。
        weight_source_path: "scenes/test/surface/weights.rgba8".to_owned(), // 作者RGBA源不参与运行时加载。
        weight_texture_path: "scenes/test/surface/weights.ktx2".to_owned(), // 真实笔划才重载这一派生纹理。
        index_source_path: "scenes/test/surface/indices.rgba32".to_owned(), // 四槽局部索引与权重同批发布。
        index_texture_path: "scenes/test/surface/indices.ktx2".to_owned(),  // 运行时整数索引派生图。
        resolution: [2, 2], // 小尺寸只用于纯JSON测试。
        layers: vec![SceneSurfaceLayer {
            id: "soil".to_owned(),        // 稳定场景局部身份属于语义内容。
            label: "土壤".to_owned(),     // 作者显示名同样属于定义。
            preset_id: "soil".to_owned(), // PBR参数改由全局可编辑预设稳定引用。
        }],
    }
}

#[test]
fn placement_water_is_validated_before_save_staging() {
    let scene_id = SceneId::parse("MAP-water-preflight").expect("测试场景ID合法");
    let bounds = Rect::from_corners(Vec2::splat(-100.0), Vec2::splat(100.0)); // 测试记录原点位于合法范围内。
    let valid = placement_document(&scene_id); // 完整稳定身份记录应通过保存前门。
    assert!(validate_placements(&valid, &scene_id, bounds).is_ok()); // 完整节点路径与有限参数允许进入事务。

    let mut invalid = valid.clone(); // 只破坏节点路径以隔离失败原因。
    invalid.placements[0].water_surfaces[0].node_path.clear(); // 只有易漂移索引的旧试验绑定必须失败关闭。
    assert!(validate_placements(&invalid, &scene_id, bounds).is_err()); // 错误发生在任何文件stage之前。
}

#[test]
fn placement_transform_is_validated_before_save_staging() {
    let scene_id = SceneId::parse("MAP-transform-preflight").expect("测试场景ID合法");
    let bounds = Rect::from_corners(Vec2::splat(-100.0), Vec2::splat(100.0)); // 旧主网格范围不再是容器边界。
    let mut document = placement_document(&scene_id); // 从水体与身份均合法的完整记录开始。
    document.placements[0].translation[0] = 50_000.0;
    validate_placements(&document, &scene_id, bounds).expect("旧范围外的有限位置也必须能够保存");
    document.placements[0].translation[0] = f32::INFINITY;
    assert!(validate_placements(&document, &scene_id, bounds).is_err()); // 取消矩形限制仍保留有限值校验。

    document.placements[0].translation[0] = 0.0; // 恢复合法位置以隔离缩放失败。
    document.placements[0].scale[1] = 0.0; // 零缩放不能产生退化正式实例。
    let scale_error =
        validate_placements(&document, &scene_id, bounds).expect_err("退化缩放不能进入保存事务");
    assert!(scale_error.contains("变换无效")); // 保存门复用运行时同一公共校验。
}

#[test]
fn placement_cutout_total_is_checked_before_save_staging_for_legacy_and_object_sources() {
    let scene_id = SceneId::parse("MAP-cutout-budget-preflight").unwrap();
    let bounds = Rect::from_corners(Vec2::ZERO, Vec2::splat(64.0));
    let draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    let mut object = draft.manifest.terrain_objects_for_edit()[0].clone();
    object.terrain.kind = SceneTerrainKind::SparseClipmap;
    object.world_min = [0.0; 2];
    object.world_max = [64.0; 2];
    let mut document = placement_document(&scene_id);
    let mut prototype = document.placements.remove(0);
    prototype.water_surfaces.clear();
    prototype.terrain_patch = Some(feathrise_scene_content::ScenePlacementTerrainPatch {
        terrain_id: Some(object.id.clone()),
        sculpted_volume: true,
        cutouts: (0..16).map(|index| feathrise_scene_content::SceneTerrainCutout {
            min: [index as f32 * 2.0, 0.0],
            max: [index as f32 * 2.0 + 1.0, 1.0],
        }).collect(),
        ..Default::default()
    });
    document.placements = vec![prototype.clone(), prototype];
    document.placements[1].id = "second-volume".into();
    let objects = [object];
    for sources in [None, Some(objects.as_slice())] {
        validate_placements_by_source(&document, &scene_id, bounds, sources)
            .expect("两个作者根共32个有效洞口仍可保存");
        let mut overflow = document.clone();
        overflow.placements[1].terrain_patch.as_mut().unwrap().cutouts.push(feathrise_scene_content::SceneTerrainCutout {
            min: [40.0, 0.0], max: [41.0, 1.0],
        });
        let error = validate_placements_by_source(&overflow, &scene_id, bounds, sources)
            .expect_err("根数仅2项时，全图第33个洞口也必须在stage之前拒绝");
        assert!(error.contains("洞口超过32项"), "{error}");
    }
}

#[test]
fn placement_water_global_budget_does_not_limit_save_staging() {
    let scene_id = SceneId::parse("MAP-water-global-budget").expect("测试场景ID合法"); // 保存文档与当前场景使用同一正式身份。
    let bounds = Rect::from_corners(Vec2::splat(-100.0), Vec2::splat(100.0)); // 所有原型记录共享合法原点。
    let prototype = placement_document(&scene_id)
        .placements
        .into_iter()
        .next()
        .expect("测试原型包含一项整网格水体"); // 复用已通过单项格式校验的合法记录。
    let placements = (0..=MAX_SCENE_PLACEMENT_WATER_SURFACES_PER_SCENE)
        .map(|index| {
            let mut placement = prototype.clone(); // 每项保持一个合法水体绑定，只改变placement稳定ID。
            placement.id = format!("placement-water-{index}"); // 唯一ID确保失败只来自全图第257项预算。
            placement // 返回当前合法记录供文档收集。
        })
        .collect(); // 闭区间构造上限加一项，且没有文件或资产访问。
    let document = ScenePlacementsAsset {
        schema_version: SCENE_SCHEMA_VERSION, // 使用当前正式格式版本绕过无关版本失败。
        scene_id: scene_id.clone(),             // 使用当前场景身份绕过无关错图失败。
        placements,                         // 第257个非空绑定必须命中文档级公共门。
    }; // 完成只超全图整网格水体预算的保存快照。

    validate_placements(&document, &scene_id, bounds).expect("全图第257项水绑定必须能保存");

}

#[test]
fn semantically_unchanged_placement_bytes_are_omitted_from_the_save_batch() {
    let scene_id = SceneId::parse("MAP-placement-no-write").expect("测试场景ID合法");
    let snapshot = placement_document(&scene_id); // 模拟与磁盘语义相同的完整摆放快照。
    let compact = serde_json::to_vec(&snapshot).expect("序列化紧凑摆放JSON");
    let pretty = serde_json::to_vec_pretty(&snapshot).expect("序列化格式化摆放JSON");

    assert!(saved_placement_bytes_match(&compact, &snapshot)); // 紧凑JSON语义完全相同。
    assert!(saved_placement_bytes_match(&pretty, &snapshot)); // 空白差异不应让事务加入placements目标。
    assert!(!saved_placement_bytes_match(b"{broken", &snapshot)); // 损坏旧文件仍需由原子事务完整覆盖。

    let mut changed = snapshot.clone(); // 只改变一项真实作者内容。
    changed.placements[0].translation[0] = 3.0; // 世界位置变化必须形成新修订。
    assert!(!saved_placement_bytes_match(&compact, &changed)); // 真实实例内容变化必须继续写盘并发布。
}

#[test]
fn surface_writes_only_publish_real_definition_or_weight_changes() {
    let scene_id = SceneId::parse("MAP-surface-write-plan").expect("测试场景ID合法");
    let snapshot = surface_document(&scene_id); // 模拟编辑器加载后未改动的完整地表定义。
    let compact = serde_json::to_vec(&snapshot).expect("序列化紧凑地表JSON");
    let pretty = serde_json::to_vec_pretty(&snapshot).expect("序列化格式化地表JSON");

    let mut empty = snapshot.clone(); // 模拟删除最后层后的完整作者定义。
    empty.layers.clear(); // 仍保留原文件路径和画布尺寸。
    assert_eq!(
        surface_write_changes(Some(&compact), &empty, true),
        (true, true)
    ); // 清空必须同时发布定义与配对纹理。
    let reloaded: SceneSurfaceAsset =
        serde_json::from_slice(&serde_json::to_vec(&empty).unwrap()).unwrap(); // 使用正式Serde格式回读空定义。
    assert!(reloaded.layers.is_empty()); // 保存重载不得自动恢复基础层。
    assert_eq!(reloaded.resolution, empty.resolution); // 清空后的画布仍可继续绘制。

    assert_eq!(
        surface_write_changes(Some(&pretty), &snapshot, false),
        (false, false),
    ); // 仅改高度时既不重写地表JSON，也不重载权重纹理。
    assert_eq!(
        surface_write_changes(Some(&compact), &snapshot, true),
        (false, true),
    ); // 真实RGBA笔划只发布权重作者源与KTX2。

    let mut changed = snapshot.clone(); // 只改变一项真实定义字段。
    changed.layers[0].preset_id = "clay".to_owned(); // 真实全局预设引用变化必须产生定义修订。
    assert_eq!(
        surface_write_changes(Some(&compact), &changed, false),
        (true, false),
    ); // 定义变化不冒充权重纹理变化。
    assert_eq!(
        surface_write_changes(Some(b"{broken"), &snapshot, false),
        (true, false),
    ); // 损坏正式JSON仍会被完整重写并发布修复值。
}

#[test]
fn nested_scene_references_save_and_reopen_without_writing_source_files() {
    use feathrise_scene_content::{SceneCatalogAsset, SceneInstanceId, SceneInstancePlacement, SceneInstanceTransform,
        SceneManifestAsset, saved_scene_referrers, validate_saved_scene_instances};
    let temporary = super::super::tests::SaveTransactionTestDirectory::new();
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    draft.placements.schema_version = SCENE_SCHEMA_VERSION;
    draft.portals.schema_version = SCENE_SCHEMA_VERSION; // 保存夹具也必须使用正式文档版本，不能沿用笔刷夹具的零版本。
    draft.manifest.terrain.kind = SceneTerrainKind::SparseClipmap;
    draft.manifest.terrain.glb_path.clear();
    draft.manifest.terrain.height_path = Some(format!("{}/terrain.sparse_height.json", draft.folder));
    draft.manifest.terrain.height_texture_path = None;
    draft.terrain_mesh = None; draft.heights = None; draft.surface = None; draft.vegetation = None; draft.water = None;
    let child = SceneId::parse("MAP-source-village").unwrap();
    let reference = SceneInstancePlacement { id: SceneInstanceId::parse("village-one").unwrap(),
        source_scene_id: child.clone(), label: "村庄".into(),
        transform: SceneInstanceTransform { translation: [40.0, 8.0, -20.0], yaw_degrees: 90.0,  ..Default::default() } };
    draft.manifest.subscenes = Arc::from([reference.clone()]);
    let mut source = draft.manifest.clone(); source.id = child.clone(); source.subscenes = Arc::from([]);
    source.terrain.height_path = Some(format!("scenes/{child}/terrain.sparse_height.json"));
    let source_folder = temporary.path.join(format!("scenes/{child}"));
    std::fs::create_dir_all(&source_folder).unwrap();
    std::fs::create_dir_all(temporary.path.join(&draft.folder)).unwrap();
    let source_manifest = source_folder.join(SCENE_MANIFEST_FILE_NAME);
    let source_bytes = serde_json::to_vec_pretty(&source).unwrap();
    std::fs::write(&source_manifest, &source_bytes).unwrap();
    let source_water = source_folder.join("water.scene_water.json");
    let source_water_bytes = b"source-owned-water-version";
    std::fs::write(&source_water, source_water_bytes).unwrap();
    let catalog: SceneCatalogAsset = serde_json::from_value(serde_json::json!({"schema_version": 2, "scenes": [
        {"id": draft.scene_id, "display_name": "父场景", "folder": draft.folder, "published": false},
        {"id": child, "display_name": "源村庄", "folder": format!("scenes/{child}"), "published": false}
    ]})).unwrap();
    std::fs::write(temporary.path.join(feathrise_scene_content::SCENE_CATALOG_ASSET_PATH), serde_json::to_vec(&catalog).unwrap()).unwrap();
    assert_eq!(validate_saved_scene_instances(&temporary.path, &catalog, &draft.manifest).unwrap().len(), 2);
    let prepared = prepare_draft_writes(&temporary.path, &draft, None).unwrap();
    commit_prepared_writes_checked(prepared.writes, |_, path| {
        assert!(!path.starts_with(&source_folder), "父图事务不可写入源图目录");
        Ok(())
    }).unwrap_or_else(|error| panic!("子场景引用保存失败：{}", error.message()));
    let reopened: SceneManifestAsset = serde_json::from_slice(&std::fs::read(temporary.path.join(&draft.folder).join(SCENE_MANIFEST_FILE_NAME)).unwrap()).unwrap();
    assert_eq!(reopened.subscenes.as_ref(), &[reference]);
    assert_eq!(std::fs::read(&source_manifest).unwrap(), source_bytes);
    assert_eq!(std::fs::read(&source_water).unwrap(), source_water_bytes);
    assert_eq!(saved_scene_referrers(&temporary.path, &catalog, &child).unwrap(), vec![draft.scene_id.clone()]);
    // 已保存的源图若反向引用父图，下一次父图保存预检必须拒绝整条循环。
    source.subscenes = Arc::from([SceneInstancePlacement { id: SceneInstanceId::parse("back").unwrap(),
        source_scene_id: draft.scene_id.clone(), label: String::new(), transform: Default::default() }]);
    std::fs::write(source_manifest, serde_json::to_vec(&source).unwrap()).unwrap();
    assert!(validate_saved_scene_instances(&temporary.path, &catalog, &reopened).unwrap_err().contains("循环"));
}

#[test]
fn clipmap_mesh_water_saves_and_reopens_without_a_terrain_glb_and_rolls_back_as_one_batch() {
    let temporary = super::super::tests::SaveTransactionTestDirectory::new();
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    draft.manifest.terrain.kind = SceneTerrainKind::SparseClipmap;
    draft.manifest.terrain.glb_path.clear();
    draft.manifest.terrain.height_path =
        Some(format!("{}/terrain.sparse_height.json", draft.folder));
    draft.manifest.terrain.height_texture_path = None;
    draft.terrain_mesh = None;
    draft.heights = None;
    draft.surface = None;
    draft.vegetation = None;
    draft.water = None; // 整网格赋水属于摆放文档，无需建立静水域或整图地形输入。
    draft.placements = placement_document(&draft.scene_id);
    let folder = temporary.path.join(&draft.folder);
    std::fs::create_dir_all(&folder).unwrap();
    let placement_path = folder.join(SCENE_PLACEMENTS_FILE_NAME);
    let manifest_path = folder.join(SCENE_MANIFEST_FILE_NAME);
    let original_placements = b"old-placement-bytes";
    let original_manifest = b"old-manifest-bytes";
    std::fs::write(&placement_path, original_placements).unwrap();
    std::fs::write(&manifest_path, original_manifest).unwrap();
    for fail in [true, false] {
        let prepared = prepare_draft_writes(&temporary.path, &draft, None).unwrap();
        assert!(prepared.placements_changed);
        let guard =
            super::super::domain_files::DraftSourceSaveGuard::prepare(&temporary.path, &draft)
                .unwrap();
        let result = commit_prepared_writes_checked(prepared.writes, |_, path| {
            guard.check(path)?;
            if fail && path == placement_path {
                return Err("测试注入Clipmap整网格水摆放发布失败".into());
            }
            Ok(())
        });
        if fail {
            assert!(result.unwrap_err().rollback_complete());
            assert_eq!(std::fs::read(&placement_path).unwrap(), original_placements);
            assert_eq!(std::fs::read(&manifest_path).unwrap(), original_manifest);
        } else {
            result.unwrap_or_else(|failure| panic!("整网格水保存重试失败：{}", failure.message()));
        }
    }
    let saved: ScenePlacementsAsset =
        serde_json::from_slice(&std::fs::read(placement_path).unwrap()).unwrap();
    let manifest: feathrise_scene_content::SceneManifestAsset =
        serde_json::from_slice(&std::fs::read(manifest_path).unwrap()).unwrap();
    assert_eq!(manifest.terrain.kind, SceneTerrainKind::SparseClipmap);
    assert!(manifest.terrain.glb_path.is_empty());
    assert!(placement_documents_equal(&saved, &draft.placements));
    assert_eq!(
        saved.placements[0].water_surfaces,
        draft.placements.placements[0].water_surfaces
    );
    assert!(!folder.join("terrain.glb").exists()); // 保存网格语义不能暗中物化稀疏地形的完整GLB。
}
