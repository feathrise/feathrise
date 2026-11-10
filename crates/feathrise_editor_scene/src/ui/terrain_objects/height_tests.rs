//! 真数值输入、同对象WIP、域重绑和正式历史回归；使用内存作者源，不写项目场景。
use super::*;
use crate::authoring::{self, EditorCommand, terrain_documents::{self, TerrainDocumentIdentity}};
use bevy::ecs::system::RunSystemOnce;
use std::sync::Arc;

#[derive(Resource)]
struct WipProbe(terrain_documents::TerrainCanvas, [u64; 4]);

fn paint_wip(draft: &mut authoring::EditorSceneDraft, folder: &str) {
    draft.surface = Some(SceneSurfaceAsset {
        schema_version: SCENE_SURFACE_SCHEMA_VERSION, scene_id: draft.scene_id.clone(), resolution: [2; 2],
        weight_source_path: format!("{folder}/surface/weights.rgba8"), weight_texture_path: format!("{folder}/surface/weights.ktx2"),
        index_source_path: format!("{folder}/surface/indices.rgba32"), index_texture_path: format!("{folder}/surface/indices.ktx2"),
        layers: vec![SceneSurfaceLayer { id: "soil".into(), label: "域修改前新刷地表".into(), preset_id: "preset-rammed-earth".into() }],
    });
    draft.surface_weights = Some(Arc::new([255, 0, 0, 0].repeat(4)));
    draft.surface_indices = Some(Arc::new(vec![0; 64]));
    draft.surface_dirty = true;
    draft.vegetation = Some(SceneVegetationAsset {
        schema_version: SCENE_SCHEMA_VERSION, scene_id: draft.scene_id.clone(),
        layers: vec![SceneVegetationLayer {
            id: "grass".into(), label: "域修改前新刷草地".into(), kind: SceneVegetationKind::Blade,
            runtime_profile: Some("ordinary_grass".into()), runtime_layer_id: Some(1), prototype_id: None,
            asset_path: "gltf/grass.glb".into(), density_source_path: format!("{folder}/vegetation/grass.r8"),
            density_texture_path: format!("{folder}/vegetation/grass.ktx2"), resolution: [2; 2],
            spacing_meters: 1.0, scale_range: [1.0; 2], max_instances: 16, seed: 7,
        }],
        authoring: Some(SceneVegetationAuthoring {
            groups: vec![SceneVegetationAuthorGroup {
                id: "base".into(), label: "基础组".into(), mode: SceneVegetationAuthorGroupMode::Additive,
                output_merge: SceneVegetationOutputMergeMode::Maximum, capacity: 1.0, claim_threshold: 0.0,
                selection_seed: 7, selection_cell_meters: 1.0, enabled: true,
            }],
            layers: vec![SceneVegetationAuthorLayer {
                id: "retained-grass".into(), label: "未保存作者草".into(), group_id: "base".into(), output_layer_id: "grass".into(),
                density_source_path: format!("{folder}/vegetation/author.r8"), resolution: [2; 2],
                enabled: true, locked: false, opacity: 1.0, priority: 0, weight: 1.0,
            }],
        }),
    });
    draft.vegetation_densities.insert("grass".into(), Arc::new(vec![173; 4]));
    draft.vegetation_author_densities.insert("retained-grass".into(), Arc::new(vec![173; 4]));
    draft.vegetation_nonzero_layers.insert("grass".into());
    draft.vegetation_dirty_layers.insert("grass".into());
    draft.vegetation_author_dirty_layers.insert("retained-grass".into());
    let mut water = authoring::water_asset_with_layer_for_tests(&draft.scene_id, folder);
    water.layers[0].resolution = [2; 2]; water.layers[0].water_level = 6.75;
    draft.water = Some(water);
    draft.water_asset_path = format!("{folder}/{SCENE_WATER_FILE_NAME}");
    draft.water_masks.insert("lake".into(), Arc::new(vec![67; 4]));
    draft.water_dirty_layers.insert("lake".into());
    draft.water_catalog_registration_required = true;
    draft.water_registration_source_hash = Some("missing".into());
    draft.vegetation_registration_source_hash = Some("missing".into());
    draft.puddle_mask = Some(Arc::new(vec![23; 4]));
    draft.puddles_dirty = true;
}

fn history(app: &mut App, undoing: bool) {
    app.init_resource::<authoring::courtyards::CourtyardAuthoringState>()
        .init_resource::<authoring::GlobalEnvironmentAuthoringState>()
        .init_resource::<authoring::WaterMaskMaterializeTask>()
        .init_resource::<authoring::SurfaceLayerEditTask>()
        .init_resource::<authoring::SurfaceBrushStroke>()
        .init_resource::<authoring::WaterBrushStroke>()
        .init_resource::<crate::ui::placement_water::PlacementInstanceIndex>()
        .init_resource::<authoring::WaterFileHandoff>();
    if !app.world().contains_resource::<authoring::EditorPreviewAssets>() {
        app.insert_resource(authoring::EditorPreviewAssets {
            portal_base_mesh: default(), portal_arrow_head_mesh: default(), portal_arrow_shaft_mesh: default(),
            portal_edge_beam_mesh: default(), portal_edge_ring_mesh: default(), portal_base_material: default(),
            portal_arrow_material: default(), portal_edge_beam_material: default(), portal_edge_ring_material: default(),
            portal_label_font: default(),
        });
    }
    {
        let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
        editor.undo_requested = undoing;
        editor.redo_requested = !undoing;
    }
    app.world_mut().run_system_once(authoring::apply_undo_redo).unwrap();
}

fn numeric_text(app: &mut App, window: Entity, value: Field, text: &str) {
    let root = field(app, value);
    click(app, window, root, MouseButton::Middle);
    assert_eq!(app.world().resource::<ActiveNumericInput>().entity, Some(root));
    key(app, window, KeyCode::Digit4, Key::Character(text.into()), Some(text));
    key(app, window, KeyCode::Enter, Key::Enter, None);
}

fn finish_domain(app: &mut App, span: f32) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        app.update();
        let session = app.world().resource::<EditorAuthoringSession>();
        let draft = session.draft.as_ref().unwrap();
        if !app.world().resource::<TerrainObjectsEditorState>().preparing()
            && !draft.terrain_documents.loading
            && draft.terrain_documents.active.as_ref().is_some_and(|active| active.definition.terrain.height_scale == span)
        {
            break;
        }
        let selected = app.world().resource::<TerrainObjectsEditorState>();
        assert!(std::time::Instant::now() < deadline,
            "域重绑应完成：{}；选择{:?}/{:?}，准备{}，载入{}，目标域{:?}，活动域{:?}，修订{}/植被{}/编译{}，历史{}/{}，作用域{}",
            app.world().resource::<SceneEditorState>().status, selected.selected, selected.selected_index,
            selected.preparing(), draft.terrain_documents.loading,
            draft.manifest.terrains.as_ref().and_then(|objects| objects.first()).map(|object| object.terrain.height_scale),
            draft.terrain_documents.active.as_ref().map(|active| active.definition.terrain.height_scale),
            session.revision, session.vegetation_revision, session.vegetation_compiled_revision,
            session.undo.len(), session.redo.len(), draft.terrain_documents.scoped_through);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

fn heightfield_page() -> (App, Entity, Arc<feathrise_terrain::TerrainMeshInput>) {
    let (mut app, window) = page();
    let mesh = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        let object = &mut Arc::make_mut(draft.manifest.terrains.as_mut().unwrap())[0];
        object.terrain.base_height = -7.5;
        object.terrain.height_scale = 20.0; // 原域中心为2.5m，不能把扩大误作几何Y缩放。
        object.terrain.glb_path = "scenes/EDGE-TEST/terrains/mesh-a/missing-old-geometry.glb".into();
        let folder = object.private_folder(&draft.scene_id);
        object.surface_path = Some(format!("{folder}/{SCENE_SURFACE_FILE_NAME}"));
        object.vegetation_path = Some(format!("{folder}/{SCENE_VEGETATION_FILE_NAME}"));
        object.water_path = Some(format!("{folder}/{SCENE_WATER_FILE_NAME}"));
        object.puddles = Some(ScenePuddleDescriptor { source_path: format!("{folder}/puddles/potential.r8"),
            texture_path: format!("{folder}/puddles/potential.ktx2"), resolution: [2; 2] });
        let object = object.clone();
        let mut mesh = (**draft.terrain_mesh.as_ref().unwrap()).clone();
        mesh.positions[60][1] = 5.75;
        mesh.positions[48][1] = -2.25;
        let index = feathrise_terrain::build_terrain_editing_index(&mesh).unwrap();
        let mesh = Arc::new(mesh);
        draft.terrain_mesh = Some(mesh.clone());
        draft.terrain_editing_index = Some(Arc::new(index));
        draft.terrain_dirty = true;
        draft.terrain_documents.active_dirty = true;
        draft.terrain_documents.active = Some(TerrainDocumentIdentity {
            definition: Arc::new(object.clone()), manifest: object.source_manifest(&draft.manifest),
            folder: object.private_folder(&draft.scene_id),
        });
        draft.terrain_documents.saved.insert(object.id.clone()); // 上次保存标记不能清掉本次活动WIP。
        paint_wip(draft, &folder);
        mesh
    };
    let session = app.world().resource::<EditorAuthoringSession>();
    let probe = terrain_documents::TerrainCanvas::snapshot(session.draft.as_ref().unwrap());
    let revisions = [session.surface_revision, session.vegetation_revision, session.vegetation_structure_revision, session.water_revision];
    app.insert_resource(WipProbe(probe, revisions));
    terrain_documents::install(&mut app);
    app.world_mut().resource_mut::<TerrainObjectsEditorState>().set_section_open(MeshPropertySection::Type, false);
    app.update();
    let tab = action(&mut app, Action::TypeProperties);
    click(&mut app, window, tab, MouseButton::Left);
    (app, window, mesh)
}

fn assert_wip(app: &App, mesh: &Arc<feathrise_terrain::TerrainMeshInput>, span: f32) {
    let session = app.world().resource::<EditorAuthoringSession>();
    let draft = session.draft.as_ref().unwrap();
    assert!(Arc::ptr_eq(draft.terrain_mesh.as_ref().unwrap(), mesh), "域事务、撤销和重做保留同一份真实顶点");
    assert_eq!(draft.terrain_mesh.as_ref().unwrap().positions, mesh.positions);
    assert!(draft.terrain_dirty && draft.terrain_documents.active_dirty);
    assert_eq!(draft.terrain_source_sha256, Some([7; 32]));
    let before = &app.world().resource::<WipProbe>().0;
    assert_eq!([session.surface_revision, session.vegetation_revision, session.vegetation_structure_revision, session.water_revision],
        app.world().resource::<WipProbe>().1, "同源域重绑不能让未改变的作者域丢失编译信用");
    assert_eq!(serde_json::to_value(&draft.surface).unwrap(), serde_json::to_value(&before.surface).unwrap());
    assert_eq!(serde_json::to_value(&draft.vegetation).unwrap(), serde_json::to_value(&before.vegetation).unwrap());
    assert_eq!(serde_json::to_value(&draft.water).unwrap(), serde_json::to_value(&before.water).unwrap());
    assert!(Arc::ptr_eq(draft.surface_weights.as_ref().unwrap(), before.surface_weights.as_ref().unwrap()));
    assert!(Arc::ptr_eq(draft.surface_indices.as_ref().unwrap(), before.surface_indices.as_ref().unwrap()));
    assert!(Arc::ptr_eq(draft.puddle_mask.as_ref().unwrap(), before.puddle_mask.as_ref().unwrap()));
    for (current, original) in [(&draft.vegetation_densities, &before.vegetation_densities),
        (&draft.vegetation_author_densities, &before.vegetation_author_densities), (&draft.water_masks, &before.water_masks)] {
        assert_eq!(current.len(), original.len());
        assert!(current.iter().all(|(id, pixels)| Arc::ptr_eq(pixels, &original[id])), "域重绑不能重读或重置任何作者像素");
    }
    assert_eq!(draft.surface_dirty, before.surface_dirty);
    assert_eq!(draft.puddles_dirty, before.puddles_dirty);
    assert_eq!(draft.vegetation_nonzero_layers, before.vegetation_nonzero_layers);
    assert_eq!(draft.vegetation_dirty_layers, before.vegetation_dirty_layers);
    assert_eq!(draft.vegetation_author_dirty_layers, before.vegetation_author_dirty_layers);
    assert_eq!(draft.water_dirty_layers, before.water_dirty_layers);
    assert_eq!(draft.water_asset_path, before.water_asset_path);
    assert_eq!(draft.water_catalog_registration_required, before.water_catalog_registration_required);
    assert_eq!(draft.water_registration_source_hash, before.water_registration_source_hash);
    assert_eq!(draft.vegetation_registration_source_hash, before.vegetation_registration_source_hash);
    let active = &draft.terrain_documents.active.as_ref().unwrap().definition;
    assert_eq!(active.terrain.height_scale, span);
    assert_eq!(active.terrain.base_height + span * 0.5, 2.5);
    assert_eq!(active.transform.scale[1], 1.0);
    let sampled = active.terrain.base_height + f32::from(draft.heights.as_ref().unwrap()[60]) / 65535.0 * span;
    assert!((sampled - 5.75).abs() <= span / 65535.0, "采样必须按新域重编码：{sampled}");
}

/// 保存域回归复用真实UI编辑结果；外部测试只能取得冻结草稿，不能旁路高度域数值事务。
pub(crate) fn height_domain_edited_draft_for_save() -> authoring::EditorSceneDraft {
    let (mut app, window, mesh) = heightfield_page();
    numeric_text(&mut app, window, Field::AuthorHeightDomain, "40");
    finish_domain(&mut app, 40.0);
    assert_wip(&app, &mesh, 40.0);
    let draft = app.world_mut().resource_mut::<EditorAuthoringSession>().draft.take().unwrap();
    draft
}

#[test]
fn height_domain_real_numeric_expands_wip_and_one_history_rebinds_exact_geometry_both_ways() {
    let (mut app, window, mesh) = heightfield_page();
    let index = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().terrain_editing_index.clone().unwrap();
    numeric_text(&mut app, window, Field::AuthorHeightDomain, "40");
    finish_domain(&mut app, 40.0);
    assert_wip(&app, &mesh, 40.0);
    assert!(Arc::ptr_eq(app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().terrain_editing_index.as_ref().unwrap(), &index));
    assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 1);
    assert!(matches!(app.world().resource::<EditorAuthoringSession>().undo[0], EditorCommand::TerrainObjectsChanged { .. }));
    history(&mut app, true);
    finish_domain(&mut app, 20.0);
    assert_wip(&app, &mesh, 20.0);
    assert_eq!(app.world().resource::<EditorAuthoringSession>().redo.len(), 1);
    history(&mut app, false);
    finish_domain(&mut app, 40.0);
    assert_wip(&app, &mesh, 40.0);
    assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 1);
}

#[test]
fn height_domain_real_numeric_shrink_contains_all_vertices_and_exclusion_keeps_old_domain() {
    let (mut app, window, mesh) = heightfield_page();
    numeric_text(&mut app, window, Field::AuthorHeightDomain, "16");
    finish_domain(&mut app, 16.0);
    assert_wip(&app, &mesh, 16.0);
    numeric_text(&mut app, window, Field::AuthorHeightDomain, "4");
    complete(&mut app);
    assert!(app.world().resource::<SceneEditorState>().status.contains("全部已雕刻几何"));
    assert_wip(&app, &mesh, 16.0);
    assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 1);
}

#[test]
fn height_domain_late_numeric_cannot_become_a_y_scale_or_another_grid_domain() {
    let (mut app, window, mesh) = heightfield_page();
    let root = field(&mut app, Field::AuthorHeightDomain);
    click(&mut app, window, root, MouseButton::Middle);
    key(&mut app, window, KeyCode::Digit4, Key::Character("40".into()), Some("40"));
    let domain = app.world().get::<NumericFieldBinding>(root).unwrap().editing.clone().unwrap();
    app.world_mut().resource_mut::<TerrainObjectsEditorState>().set_section_open(MeshPropertySection::Type, false);
    app.update();
    let scale = field(&mut app, Field::Selected(4));
    let target = app.world().get::<NumericFieldBinding>(scale).unwrap().current.clone().unwrap();
    assert_ne!(domain, target, "相同数字下标4也必须区分源域与无量纲Y缩放");
    late_commit(&mut app, root, 40.0);
    assert_eq!(current_object(&app).transform.scale[1], 1.0);
    assert_eq!(current_object(&app).terrain.height_scale, 20.0);
    assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
    assert!(Arc::ptr_eq(app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().terrain_mesh.as_ref().unwrap(), &mesh));
    {
        let mut state = app.world_mut().resource_mut::<TerrainObjectsEditorState>();
        state.selected = Some(TerrainId::parse("mesh-b").unwrap()); state.selected_index = Some(1);
        state.set_section_open(MeshPropertySection::Type, true);
    }
    late_commit(&mut app, root, 60.0);
    assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
    assert_eq!(app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().manifest.terrains.as_ref().unwrap()[1].terrain.height_scale, 65535.0);
}

#[test]
fn voxel_height_real_numeric_is_world_meters_and_history_keeps_all_author_sources() {
    let (mut app, window) = page();
    configure_native(&mut app, SceneTerrainKind::Voxel, 4);
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        Arc::make_mut(draft.manifest.terrains.as_mut().unwrap())[0].transform.scale[1] = 1.75;
        draft.vegetation_author_densities.insert("retained-grass".into(), Arc::new(vec![173; 16]));
    }
    let before = current_object(&app);
    app.update();
    app.world_mut().resource_mut::<TerrainObjectsEditorState>().set_section_open(MeshPropertySection::Type, false);
    app.update();
    let tab = action(&mut app, Action::TypeProperties);
    click(&mut app, window, tab, MouseButton::Left);
    assert_eq!(numeric_value(&mut app, Field::VoxelHeight), 28.0);
    numeric_text(&mut app, window, Field::VoxelHeight, "42");
    complete(&mut app);
    let after = current_object(&app);
    assert_eq!(after.transform.scale[1], 2.625);
    assert_eq!(after.voxel, before.voxel);
    assert_eq!(after.terrain, before.terrain);
    assert_eq!(after.surface_path, before.surface_path);
    assert_eq!(after.vegetation_path, before.vegetation_path);
    assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 1);
    history(&mut app, true);
    app.update();
    assert_eq!(current_object(&app), before);
    history(&mut app, false);
    app.update();
    assert_eq!(current_object(&app), after);
    assert_eq!(app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().vegetation_author_densities["retained-grass"].as_slice(), &[173; 16]);
}

#[test]
fn height_domain_clipmap_properties_have_no_fixed_cap_control() {
    let (mut app, window) = native_page(SceneTerrainKind::SparseClipmap, 4);
    app.world_mut().resource_mut::<TerrainObjectsEditorState>().set_section_open(MeshPropertySection::Type, false);
    app.update();
    let tab = action(&mut app, Action::TypeProperties);
    click(&mut app, window, tab, MouseButton::Left);
    for field in [Field::AuthorHeightDomain, Field::VoxelHeight] {
        let entity = self::field(&mut app, field);
        assert!(app.world().entity(entity).contains::<NumericInputDisabled>());
        assert!(numeric::field_value(field, app.world().resource::<EditorAuthoringSession>(), app.world().resource::<TerrainObjectsEditorState>()).is_none());
    }
    let world = app.world_mut();
    let summary = world.query_filtered::<&Text, With<SourceSummary>>().single(world).unwrap();
    assert!(summary.0.contains("不受创建高度域限制"));
    assert!(!summary.0.contains("作者高度域："));
}
