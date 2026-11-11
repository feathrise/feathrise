//! 对象作者槽必须同时交换像素、来源和脏责任；历史始终绑定最初绘制的对象。
use super::*;
use crate::authoring::EditorCommand;
use feathrise_scene_content::TerrainId;
use std::sync::Arc;

#[path = "root_water_history_tests.rs"]
mod root_water_history_tests;

#[test]
fn height_domain_cached_canvas_rebind_keeps_parked_unsaved_geometry_and_pixels() {
    use bevy::prelude::*;
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    draft.manifest.terrain.base_height = -20.0;
    draft.manifest.terrain.height_scale = 40.0;
    draft.manifest.terrain.glb_path = "scenes/EDGE-TEST/missing-domain-baseline.glb".into();
    Arc::make_mut(draft.terrain_mesh.as_mut().unwrap()).positions[60][1] = 12.5;
    draft.terrain_dirty = true;
    draft.surface_weights = Some(Arc::new(vec![93; 16]));
    draft.surface_dirty = true;
    ensure_primary_canvas(&mut draft);
    let mesh = draft.terrain_mesh.clone().unwrap();
    let pixels = draft.surface_weights.clone().unwrap();
    let mut parked = TerrainCanvas::default();
    parked.swap(&mut draft);
    for span in [80.0, 40.0, 80.0] {
        let mut identity = parked.identity.clone().unwrap();
        let mut object = (*identity.definition).clone();
        object.terrain.base_height = -span * 0.5;
        object.terrain.height_scale = span;
        identity.manifest.terrain = object.terrain.clone();
        identity.definition = Arc::new(object);
        parked = bevy::tasks::block_on(super::load::load(identity, Some(Arc::new(parked)), false, false)).unwrap();
        assert!(Arc::ptr_eq(parked.terrain_mesh.as_ref().unwrap(), &mesh));
        assert!(Arc::ptr_eq(parked.surface_weights.as_ref().unwrap(), &pixels));
        assert!(parked.terrain_dirty && parked.surface_dirty);
        assert_eq!(parked.terrain_source_sha256, Some([7; 32]));
        let sampled = -span * 0.5 + f32::from(parked.heights.as_ref().unwrap()[60]) / 65535.0 * span;
        assert!((sampled - 12.5).abs() <= span / 65535.0);
    }
}

#[test]
fn cross_mesh_partial_canvas_focus_load_preserves_unsaved_grass_instead_of_rereading_disk() {
    let mut app=bevy::prelude::App::new();app.add_plugins(bevy::prelude::MinimalPlugins);
    let draft=crate::authoring::vegetation_paint_scope::painted_draft();
    let mut canvas=(**draft.terrain_documents.values.values().next().unwrap()).clone();
    canvas.vegetation_only=true;
    let pixels=canvas.vegetation_author_densities.clone();
    let mut identity=canvas.identity.clone().unwrap();
    let mut object=(*identity.definition).clone();object.terrain.kind=feathrise_scene_content::SceneTerrainKind::SparseClipmap;
    object.surface_path=None;object.water_path=None;
    object.vegetation_path=Some(format!("{}/missing-on-disk.scene_vegetation.json",identity.folder));
    identity.definition=Arc::new(object);canvas.identity=Some(identity.clone());
    let loaded=bevy::tasks::block_on(super::load::load(identity,Some(Arc::new(canvas)),false,false)).unwrap();
    assert!(!loaded.vegetation_only);
    for (id,source) in pixels {assert!(Arc::ptr_eq(&loaded.vegetation_author_densities[&id],&source));}
}

fn vegetation_fixture(
    draft: &crate::authoring::EditorSceneDraft,
) -> feathrise_scene_content::SceneVegetationAsset {
    use feathrise_scene_content::*;
    let folder = draft.terrain_folder();
    SceneVegetationAsset {
        schema_version: SCENE_SCHEMA_VERSION,
        scene_id: draft.scene_id.clone(),
        layers: vec![SceneVegetationLayer {
            id: "grass".into(),
            label: "草".into(),
            kind: SceneVegetationKind::Blade,
            runtime_profile: Some("ordinary_grass".into()),
            runtime_layer_id: Some(1),
            prototype_id: None,
            asset_path: "grass.glb".into(),
            density_source_path: format!("{folder}/vegetation/grass.r8"),
            density_texture_path: format!("{folder}/vegetation/grass.ktx2"),
            resolution: [4, 4],
            spacing_meters: 1.0,
            scale_range: [1.0, 1.0],
            max_instances: 16,
            seed: 1,
        }],
        authoring: Some(SceneVegetationAuthoring {
            groups: vec![SceneVegetationAuthorGroup {
                id: "base".into(),
                label: "基础组".into(),
                mode: SceneVegetationAuthorGroupMode::Additive,
                output_merge: SceneVegetationOutputMergeMode::Maximum,
                capacity: 1.0,
                claim_threshold: 0.0,
                selection_seed: 1,
                selection_cell_meters: 1.0,
                enabled: true,
            }],
            layers: vec![SceneVegetationAuthorLayer {
                id: "author".into(),
                label: "作者草".into(),
                group_id: "base".into(),
                output_layer_id: "grass".into(),
                density_source_path: format!("{folder}/vegetation/author.r8"),
                resolution: [4, 4],
                enabled: true,
                locked: false,
                opacity: 1.0,
                priority: 0,
                weight: 1.0,
            }],
        }),
    }
}

#[derive(bevy::prelude::Resource)]
struct ParkProbe {
    target: Arc<feathrise_scene_content::SceneTerrainObject>,
    parked: bool,
    rejected: usize,
}

fn try_park_after_compile(
    idle: resources::CanvasIdle,
    mut session: bevy::prelude::ResMut<crate::authoring::EditorAuthoringSession>,
    mut probe: bevy::prelude::ResMut<ParkProbe>,
) {
    if probe.parked {
        return;
    }
    if idle.busy(&session) {
        probe.rejected += 1;
        return;
    }
    let draft = session.draft.as_mut().unwrap();
    let mut previous = TerrainCanvas::default();
    previous.swap(draft);
    let id = previous.identity.as_ref().unwrap().definition.id.clone();
    draft.terrain_documents.dirty_stored.insert(id.clone());
    draft
        .terrain_documents
        .values
        .insert(id, Arc::new(previous));
    draft.terrain_documents.active = Some(TerrainDocumentIdentity {
        manifest: probe.target.source_manifest(&draft.manifest),
        folder: probe.target.private_folder(&draft.scene_id),
        definition: probe.target.clone(),
    });
    session.vegetation_revision = session.vegetation_revision.wrapping_add(1);
    probe.parked = true;
}

#[test]
fn painted_canvas_cannot_park_until_real_compile_publishes_its_formal_output() {
    use crate::authoring::vegetation_compile::{
        VegetationCompileState, poll_vegetation_compile, start_vegetation_compile,
    };
    use crate::authoring::{EditorAuthoringSession, VegetationBrushStroke};
    use bevy::prelude::*;
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    let mut first = draft.manifest.terrain_objects_for_edit()[0].clone();
    first.id = TerrainId::parse("canvas-a").unwrap();
    let mut second = first.clone();
    second.id = TerrainId::parse("canvas-b").unwrap();
    draft.manifest.terrain = feathrise_scene_content::SceneTerrainDescriptor::empty();
    draft.manifest.terrains = Some(Arc::from([first.clone(), second.clone()]));
    draft.terrain_documents.active = Some(TerrainDocumentIdentity {
        manifest: first.source_manifest(&draft.manifest),
        folder: first.private_folder(&draft.scene_id),
        definition: Arc::new(first.clone()),
    });
    draft.vegetation = Some(vegetation_fixture(&draft));
    draft
        .vegetation_author_densities
        .insert("author".into(), Arc::new(vec![173; 16]));
    draft
        .vegetation_densities
        .insert("grass".into(), Arc::new(vec![0; 16]));
    draft.vegetation_author_dirty_layers.insert("author".into());
    app.insert_resource(EditorAuthoringSession {
        draft: Some(draft),
        vegetation_revision: 1,
        vegetation_compiled_revision: 0,
        ..default()
    })
    .init_resource::<crate::resources::SceneEditorState>()
    .init_resource::<VegetationBrushStroke>()
    .init_resource::<VegetationCompileState>()
    .insert_resource(ParkProbe {
        target: Arc::new(second),
        parked: false,
        rejected: 0,
    })
    .add_systems(
        Update,
        (
            try_park_after_compile,
            start_vegetation_compile,
            poll_vegetation_compile,
        )
            .chain(),
    );
    app.update();
    assert!(
        !app.world().resource::<ParkProbe>().parked,
        "首帧必须等待作者A的完整编译，不能因选择B而放行"
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !app.world().resource::<ParkProbe>().parked {
        app.update();
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let session = app.world().resource::<EditorAuthoringSession>();
    let draft = session.draft.as_ref().unwrap();
    let cached = &draft.terrain_documents.values[&first.id];
    assert!(
        cached.vegetation_densities["grass"]
            .iter()
            .all(|value| *value == 173)
    );
    assert!(
        cached.vegetation_dirty_layers.contains("grass"),
        "保存投影必须带新正式输出的写盘责任"
    );
    assert!(cached.vegetation_author_dirty_layers.contains("author"));
    assert_eq!(
        draft
            .terrain_documents
            .active
            .as_ref()
            .unwrap()
            .definition
            .id
            .as_str(),
        "canvas-b"
    );
    assert!(app.world().resource::<ParkProbe>().rejected > 0);
}

#[test]
fn compile_gate_allows_clean_initialization_and_no_vegetation_but_protects_last_author_removal() {
    use bevy::{ecs::system::RunSystemOnce, prelude::*};
    fn busy(
        idle: resources::CanvasIdle,
        session: Res<crate::authoring::EditorAuthoringSession>,
    ) -> bool {
        idle.busy(&session)
    }
    let mut app = App::new();
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    let object = draft.manifest.terrain_objects_for_edit()[0].clone();
    draft.terrain_documents.active = Some(TerrainDocumentIdentity {
        manifest: draft.manifest.clone(),
        folder: draft.folder.clone(),
        definition: Arc::new(object),
    });
    draft.vegetation = Some(vegetation_fixture(&draft));
    app.insert_resource(crate::authoring::EditorAuthoringSession {
        draft: Some(draft),
        vegetation_revision: 8,
        vegetation_compiled_revision: 0,
        ..default()
    });
    assert!(
        !app.world_mut().run_system_once(busy).unwrap(),
        "干净画布的初始化编译不阻止切换"
    );
    {
        let mut session = app
            .world_mut()
            .resource_mut::<crate::authoring::EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        draft.terrain_documents.active_dirty = true;
        draft
            .vegetation
            .as_mut()
            .unwrap()
            .authoring
            .as_mut()
            .unwrap()
            .layers
            .clear();
    }
    assert!(
        app.world_mut().run_system_once(busy).unwrap(),
        "删除末作者层必须等正式输出清零，即便作者脏集合为空"
    );
    app.world_mut()
        .resource_mut::<crate::authoring::EditorAuthoringSession>()
        .vegetation_compiled_revision = 8;
    assert!(!app.world_mut().run_system_once(busy).unwrap());
    {
        let mut session = app
            .world_mut()
            .resource_mut::<crate::authoring::EditorAuthoringSession>();
        session.vegetation_revision = 9;
        let draft = session.draft.as_mut().unwrap();
        draft.terrain_documents.active = None;
        draft.terrain_documents.active_dirty = false;
        draft.vegetation_author_dirty_layers.insert("author".into());
    }
    assert!(
        app.world_mut().run_system_once(busy).unwrap(),
        "旧主画布尚未认领身份时，也必须等待真实作者脏层编译完成"
    );
    {
        let mut session = app
            .world_mut()
            .resource_mut::<crate::authoring::EditorAuthoringSession>();
        session.vegetation_revision = 9;
        session.draft.as_mut().unwrap().vegetation = None;
    }
    assert!(
        !app.world_mut().run_system_once(busy).unwrap(),
        "无植被不能因修订差异永久等待"
    );
}
#[test]
fn switching_canvas_preserves_each_objects_pixels_and_dirty_responsibility() {
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    let object = draft.manifest.terrain_objects_for_edit()[0].clone();
    draft.terrain_documents.active = Some(TerrainDocumentIdentity {
        definition: Arc::new(object),
        manifest: draft.manifest.clone(),
        folder: draft.folder.clone(),
    });
    let first = Arc::new(vec![7u8; 16]);
    draft.puddle_mask = Some(first.clone());
    draft.puddles_dirty = true;
    let mut parked = TerrainCanvas::default();
    parked.swap(&mut draft);
    assert!(draft.puddle_mask.is_none());
    assert!(parked.dirty());
    let second = Arc::new(vec![11u8; 16]);
    draft.puddle_mask = Some(second.clone());
    parked.swap(&mut draft);
    assert!(Arc::ptr_eq(draft.puddle_mask.as_ref().unwrap(), &first));
    assert!(Arc::ptr_eq(parked.puddle_mask.as_ref().unwrap(), &second));
    assert!(draft.puddles_dirty);
    assert!(!parked.puddles_dirty);
}
#[test]
fn canvas_history_is_scoped_without_reordering_scene_history() {
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    let mut object = draft.manifest.terrain_objects_for_edit()[0].clone();
    object.id = TerrainId::parse("canvas-b").unwrap();
    draft.terrain_documents.active = Some(TerrainDocumentIdentity {
        definition: Arc::new(object),
        manifest: draft.manifest.clone(),
        folder: draft.folder.clone(),
    });
    let mut session = crate::authoring::EditorAuthoringSession {
        draft: Some(draft),
        ..Default::default()
    };
    let collection =
        feathrise_scene_content::SceneTerrainCollection::capture(&session.draft.as_ref().unwrap().manifest);
    session.undo.push(EditorCommand::TerrainObjectsChanged {
        before: collection.clone(),
        after: collection,
    });
    session.undo.push(EditorCommand::WaterLevelChanged {
        layer_id: "lake".into(),
        before: 0.0,
        after: 2.0,
    });
    scope_history(&mut session);
    assert!(matches!(
        &session.undo[0],
        EditorCommand::TerrainObjectsChanged { .. }
    ));
    assert!(
        matches!(&session.undo[1], EditorCommand::TerrainCanvas { id, command }
        if id.as_str() == "canvas-b" && matches!(command.as_ref(), EditorCommand::WaterLevelChanged { .. }))
    );
    assert!(session.draft.as_ref().unwrap().stored_terrain_changes());
    assert!(crate::authoring::command_uses_bounded_water_history(
        &session.undo[1]
    ));
    scope_history(&mut session);
    assert_eq!(session.undo.len(), 2);
}

#[test]
fn undoing_first_object_migration_restores_primary_pixels_and_private_paths() {
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    let original = draft.manifest.clone();
    let primary = original.terrain_objects_for_edit()[0].clone();
    draft.terrain_documents.active = Some(TerrainDocumentIdentity {
        definition: Arc::new(primary.clone()),
        manifest: original.clone(),
        folder: draft.folder.clone(),
    });
    let first = Arc::new(vec![7u8; 16]);
    draft.puddle_mask = Some(first.clone());
    draft.puddles_dirty = true;
    super::transitions::park(&mut draft);
    let mut second = primary;
    second.id = TerrainId::parse("canvas-b").unwrap();
    let second_folder = second.private_folder(&draft.scene_id);
    let mut second_manifest = original.clone();
    second_manifest.world_min = [100.; 2];
    second_manifest.world_max = [120.; 2];
    draft.manifest.terrains = Some(Arc::from([second.clone()]));
    draft.terrain_documents.active = Some(TerrainDocumentIdentity {
        definition: Arc::new(second),
        manifest: second_manifest,
        folder: second_folder,
    });
    let second_pixels = Arc::new(vec![9u8; 16]);
    draft.puddle_mask = Some(second_pixels.clone());
    draft.puddles_dirty = true;
    draft.manifest = original.clone();
    restore_legacy(&mut draft).unwrap();
    assert!(draft.terrain_documents.active.is_none());
    assert!(Arc::ptr_eq(draft.puddle_mask.as_ref().unwrap(), &first));
    assert_eq!(draft.terrain_manifest().world_min, original.world_min);
    assert_eq!(draft.terrain_folder(), draft.folder);
    assert!(Arc::ptr_eq(
        draft.terrain_documents.values[&TerrainId::parse("canvas-b").unwrap()]
            .puddle_mask
            .as_ref()
            .unwrap(),
        &second_pixels
    ));
    assert!(
        draft
            .terrain_documents
            .dirty_stored
            .contains(&TerrainId::parse("canvas-b").unwrap())
    );
}

#[test]
fn legacy_geometry_history_is_bound_before_switching_to_another_object() {
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    let object = draft.manifest.terrain_objects_for_edit()[0].clone();
    draft.terrain_documents.active = Some(TerrainDocumentIdentity {
        definition: Arc::new(object),
        manifest: draft.manifest.clone(),
        folder: draft.folder.clone(),
    });
    let mut session = crate::authoring::EditorAuthoringSession {
        draft: Some(draft),
        ..Default::default()
    };
    session.undo.push(EditorCommand::TerrainChanged(Vec::new()));
    scope_history(&mut session);
    assert!(
        matches!(&session.undo[0],EditorCommand::TerrainCanvas {id,command} if *id==TerrainId::legacy()
        && matches!(command.as_ref(),EditorCommand::TerrainChanged(_)))
    );
}

#[test]
fn restoring_a_saved_primary_canvas_uses_its_new_glb_baseline() {
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    let original = draft.manifest.clone();
    draft.terrain_source_sha256 = Some([1; 32]);
    super::ensure_primary_canvas(&mut draft);
    super::transitions::park(&mut draft);
    let mut other = original.terrain_objects_for_edit()[0].clone();
    other.id = TerrainId::parse("other-canvas").unwrap();
    draft.terrain_documents.active = Some(TerrainDocumentIdentity {
        folder: other.private_folder(&draft.scene_id),
        manifest: other.source_manifest(&original),
        definition: Arc::new(other),
    });
    draft.terrain_documents.saved.insert(TerrainId::legacy());
    draft
        .terrain_documents
        .saved_digests
        .insert(TerrainId::legacy(), [2; 32]);
    restore_legacy(&mut draft).unwrap();
    assert_eq!(draft.terrain_source_sha256, Some([2; 32]));
    assert!(!draft.terrain_dirty);
    assert_eq!(
        draft.terrain_manifest().terrain.glb_path,
        original.terrain.glb_path
    );
}

#[test]
fn legacy_metadata_migration_binds_existing_previews_to_the_primary_instance_without_io() {
    use bevy::prelude::*;
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    draft.terrain_mesh = Some(Arc::new(feathrise_terrain::TerrainMeshInput {
        positions: vec![[-5., 0., -5.], [5., 0., -5.], [5., 0., 5.], [-5., 0., 5.]],
        indices: vec![0, 2, 1, 0, 3, 2],
    }));
    super::ensure_primary_canvas(&mut draft);
    let mut primary = draft.manifest.terrain_objects_for_edit()[0].clone();
    primary.display_name = "重命名的主地形".into();
    draft.manifest.terrain = feathrise_scene_content::SceneTerrainDescriptor::empty();
    draft.manifest.terrains = Some(Arc::from([primary.clone()]));
    let owner = draft.scene_id.clone();
    let mut selection = crate::ui::terrain_objects::TerrainObjectsEditorState::default();
    selection.selected = Some(primary.id.clone());
    selection.selected_index = Some(0);
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(crate::resources::SceneEditorState {
            open: true,
            ..default()
        })
        .insert_resource(selection)
        .insert_resource(crate::authoring::EditorAuthoringSession {
            draft: Some(draft),
            ..default()
        })
        .init_resource::<TerrainDocumentTask>()
        .init_resource::<feathrise_scene::TerrainObjectAuthoringTarget>()
        .init_resource::<ButtonInput<MouseButton>>()
        .add_systems(Update, super::systems::focus);
    app.update();
    let target = app
        .world()
        .resource::<feathrise_scene::TerrainObjectAuthoringTarget>();
    assert_eq!(target.scene_id, Some(owner));
    assert_eq!(target.terrain, Some(primary.id.clone()));
    assert!(
        app.world()
            .resource::<TerrainDocumentTask>()
            .pending
            .is_none()
    );
    let active = app
        .world()
        .resource::<crate::authoring::EditorAuthoringSession>()
        .draft
        .as_ref()
        .unwrap()
        .terrain_documents
        .active
        .as_ref()
        .unwrap();
    assert_eq!(active.definition.display_name, primary.display_name);
    app.update(); // 稳定帧仍保持同一实例目标，不能回到空容器。
    assert_eq!(
        app.world()
            .resource::<feathrise_scene::TerrainObjectAuthoringTarget>()
            .terrain,
        Some(primary.id)
    );
}

fn explicit_primary_focus_fixture(cached: bool) -> bevy::prelude::App {
    use bevy::prelude::*;
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    let mut primary = draft.manifest.terrain_objects_for_edit()[0].clone();
    primary.terrain.kind = feathrise_scene_content::SceneTerrainKind::SparseClipmap;
    primary.terrain.glb_path.clear();
    primary.terrain.height_path = Some(feathrise_scene_content::sparse_height_manifest_asset_path(&draft.scene_id));
    primary.surface_path = None;
    primary.water_path = None;
    primary.vegetation_path = Some(format!("{}/missing-primary-focus.scene_vegetation.json", draft.folder));
    draft.manifest.terrain = feathrise_scene_content::SceneTerrainDescriptor::empty();
    draft.manifest.terrains = Some(Arc::from([primary.clone()]));
    draft.terrain_mesh = None;
    draft.terrain_editing_index = None;
    draft.heights = None;
    if cached {
        let mut canvas = TerrainCanvas {
            identity: Some(TerrainDocumentIdentity {
                definition: Arc::new(primary.clone()),
                manifest: primary.source_manifest(&draft.manifest),
                folder: draft.folder.clone(),
            }),
            vegetation: Some(vegetation_fixture(&draft)),
            ..default()
        };
        canvas.vegetation_author_densities.insert("author".into(), Arc::new(vec![173; 16]));
        canvas.vegetation_densities.insert("grass".into(), Arc::new(vec![93; 16]));
        draft.terrain_documents.values.insert(primary.id.clone(), Arc::new(canvas));
    }
    let mut selection = crate::ui::terrain_objects::TerrainObjectsEditorState::default();
    selection.selected = Some(primary.id);
    selection.selected_index = Some(0);
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(crate::resources::SceneEditorState { open: true, ..default() })
        .insert_resource(selection)
        .insert_resource(crate::authoring::EditorAuthoringSession { draft: Some(draft), ..default() })
        .init_resource::<TerrainDocumentTask>()
        .init_resource::<feathrise_scene::TerrainObjectAuthoringTarget>()
        .init_resource::<ButtonInput<MouseButton>>()
        .add_systems(Update, super::systems::focus);
    app
}

#[test]
fn explicit_primary_cold_focus_reads_declared_documents_before_granting_canvas() {
    let mut app = explicit_primary_focus_fixture(false);
    app.update();
    let session = app.world().resource::<crate::authoring::EditorAuthoringSession>();
    let draft = session.draft.as_ref().unwrap();
    assert!(draft.terrain_documents.active.is_none() && draft.terrain_documents.loading);
    assert!(draft.vegetation.is_none());
    assert!(app.world().resource::<feathrise_scene::TerrainObjectAuthoringTarget>().terrain.is_none());
    let pending = app.world_mut().resource_mut::<TerrainDocumentTask>().pending.take().unwrap();
    let error = bevy::tasks::block_on(pending.task).err().expect("声明的文件缺失必须真实读盘失败，不能认领空槽成功");
    assert!(error.contains("missing-primary-focus.scene_vegetation.json"));
}

#[test]
fn explicit_primary_cached_focus_adopts_complete_vegetation_pixels_instead_of_empty_container() {
    let mut app = explicit_primary_focus_fixture(true);
    app.update();
    assert!(app.world().resource::<TerrainDocumentTask>().pending.is_some());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while app.world().resource::<TerrainDocumentTask>().pending.is_some() {
        app.update();
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let session = app.world().resource::<crate::authoring::EditorAuthoringSession>();
    let draft = session.draft.as_ref().unwrap();
    assert_eq!(draft.terrain_documents.active.as_ref().unwrap().definition.id, TerrainId::legacy());
    assert!(!draft.terrain_documents.loading);
    assert_eq!(draft.vegetation.as_ref().unwrap().authoring.as_ref().unwrap().layers.len(), 1);
    assert_eq!(draft.vegetation_author_densities["author"].as_slice(), &[173; 16]);
    assert_eq!(draft.vegetation_densities["grass"].as_slice(), &[93; 16]);
    assert_eq!(session.revision, 1);
}

/// 新建静水通过真实画布load/采用系统续接；缓存只替代磁盘夹具，不直接伪造完成回执。
fn auto_water_canvas_fixture(cached: bool) -> (bevy::prelude::App, Arc<TerrainCanvas>) {
    use bevy::prelude::*;
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    let mut object = draft.manifest.terrain_objects_for_edit()[0].clone();
    object.id = TerrainId::parse("auto-water-canvas-integration").unwrap();
    let folder = object.private_folder(&draft.scene_id);
    object.water_path = Some(format!("{folder}/missing-auto-water.scene_water.json"));
    draft.manifest.terrain = feathrise_scene_content::SceneTerrainDescriptor::empty();
    draft.manifest.terrains = Some(Arc::from([object.clone()]));
    draft.terrain_documents.active = Some(TerrainDocumentIdentity {
        definition: Arc::new(object.clone()), manifest: object.source_manifest(&draft.manifest), folder,
    });
    draft.water = Some(crate::authoring::default_water_asset(&draft.scene_id));
    draft.water_asset_path = object.water_path.clone().unwrap();
    let mut canvas = TerrainCanvas::default();
    canvas.swap(&mut draft);
    let canvas = Arc::new(canvas);
    if cached { draft.terrain_documents.values.insert(object.id, canvas.clone()); }
    let mut selection = crate::ui::terrain_objects::TerrainObjectsEditorState::default();
    selection.scene_id = Some(draft.scene_id.clone());
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(crate::resources::SceneEditorState {
            open: true, selected_scene_id: Some(draft.scene_id.clone()),
            water_selected_preset_id: Some("windy_lake".into()), ..default()
        })
        .insert_resource(crate::authoring::EditorAuthoringSession { draft: Some(draft), revision: 9, ..default() })
        .insert_resource(selection)
        .init_resource::<TerrainDocumentTask>()
        .init_resource::<feathrise_scene::TerrainObjectAuthoringTarget>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<crate::resources::SceneEditorNumericScrubMouseGuard>()
        .init_resource::<crate::authoring::WaterBrushStroke>()
        .init_resource::<crate::authoring::WaterNetworkEditState>()
        .init_resource::<crate::ui::water_layers::PendingClipmapWaterLayerCreate>()
        .init_resource::<crate::viewport::LayerCandidateState>()
        .add_systems(Update, (super::systems::focus, crate::ui::water_layers::handle_water_layer_buttons).chain());
    (app, canvas)
}

fn click_auto_water_once(app: &mut bevy::prelude::App) {
    use bevy::ecs::system::RunSystemOnce;
    use crate::components::SceneEditorActionButton;
    let button = app.world_mut().spawn((feathrise_editor_ui::EditorButtonClick::triggered(),
        SceneEditorActionButton(crate::types::EditorAction::WaterAddLayer))).id();
    app.world_mut().run_system_once(crate::ui::water_layers::handle_water_layer_buttons).unwrap();
    app.world_mut().despawn(button); // 后续帧没有第二次按钮边沿，自动续接必须自己完成。
}

#[test]
fn new_root_water_ignores_cached_or_missing_terrain_documents_without_stealing_properties() {
    for cached in [true, false] {
        let (mut app, canvas) = auto_water_canvas_fixture(cached);
        let keep_properties = crate::viewport::LayerTarget::Boundary("keep-existing-properties".into());
        app.world_mut().resource_mut::<crate::viewport::LayerCandidateState>().selected = Some(keep_properties.clone());
        click_auto_water_once(&mut app);
        assert!(app.world().resource::<crate::ui::terrain_objects::TerrainObjectsEditorState>().selected.is_none());
        assert!(app.world().resource::<TerrainDocumentTask>().requested.is_none());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            app.update();
            if app.world().resource::<crate::authoring::EditorAuthoringSession>().draft.as_ref().unwrap()
                .water.as_ref().is_some_and(|water| water.layers.len() == 1) { break; }
            assert!(std::time::Instant::now() < deadline, "新建根水不应读取缺失地形文件");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let session = app.world().resource::<crate::authoring::EditorAuthoringSession>();
        let draft = session.draft.as_ref().unwrap();
        let water = draft.water.as_ref().unwrap();
        assert!(draft.terrain_documents.active.is_none() && !draft.terrain_documents.loading);
        assert_eq!(water.layers[0].preset_id, "windy_lake");
        assert!(water.layers[0].mask_source_path.starts_with(&format!("{}/water/", draft.folder)));
        assert!(!water.layers[0].mask_source_path.starts_with(&canvas.identity.as_ref().unwrap().folder));
        assert_eq!(session.undo.len(), 1);
        assert_eq!(session.revision, 10, "只有水层正式采用推进修订，不加载网格");
        assert_eq!(app.world().resource::<crate::viewport::LayerCandidateState>().selected.as_ref(), Some(&keep_properties));
        assert!(app.world().resource::<feathrise_scene::TerrainObjectAuthoringTarget>().terrain.is_none());
    }
}

#[test]
fn root_and_private_water_swap_preserves_paths_masks_dirty_responsibility_and_revision() {
    let mut draft = crate::ui::water_layers::tests::empty_root_created_via_button_for_tests();
    let root_path = draft.water_asset_path.clone();
    let root_id = draft.water.as_ref().unwrap().layers[0].id.clone();
    let root_pixels = draft.water_masks[&root_id].clone();
    super::root_water::remember_revision(&mut draft, 31);
    let mut object = crate::authoring::test_heightfield_draft(Arc::new(vec![0;121])).manifest.terrain_objects_for_edit()[0].clone();
    object.id = TerrainId::parse("private-water").unwrap();
    let folder = object.private_folder(&draft.scene_id);
    object.water_path = Some(format!("{folder}/water.scene_water.json"));
    let private = crate::authoring::water_asset_with_layer_for_tests(&draft.scene_id, &folder);
    let private_id = private.layers[0].id.clone();
    let private_pixels = Arc::new(vec![93; 512*512]);
    draft.manifest.terrains = Some(Arc::from([object.clone()]));
    let mut canvas = TerrainCanvas { identity: Some(TerrainDocumentIdentity {
        definition: Arc::new(object.clone()), manifest: object.source_manifest(&draft.manifest), folder }),
        water: Some(private), water_asset_path: object.water_path.clone().unwrap(),
        water_masks: std::collections::BTreeMap::from([(private_id.clone(), private_pixels.clone())]),
        water_dirty_layers: std::collections::BTreeSet::from([private_id.clone()]), ..Default::default() };
    super::transitions::park(&mut draft);
    canvas.swap(&mut draft);
    let root = super::root_water::source(&draft);
    assert_eq!(root.asset_path, root_path);
    assert!(root.dirty_layers.contains(&root_id));
    assert!(Arc::ptr_eq(&root.masks[&root_id], &root_pixels));
    assert_eq!(super::root_water::revision(&draft, 900), 31, "private笔划不改变停放根修订");
    assert!(draft.stored_terrain_changes());
    assert!(super::root_water::focus(&mut draft));
    assert!(draft.terrain_documents.active.is_none());
    assert_eq!(draft.water_asset_path, root_path);
    assert!(Arc::ptr_eq(&draft.water_masks[&root_id], &root_pixels));
    let parked_private = &draft.terrain_documents.values[&object.id];
    assert_eq!(parked_private.water_asset_path, object.water_path.unwrap());
    assert!(Arc::ptr_eq(&parked_private.water_masks[&private_id], &private_pixels));
    assert!(parked_private.water_dirty_layers.contains(&private_id));
}

#[test]
fn root_json_only_edits_keep_save_responsibility_after_parking_and_failed_handoff() {
    for delete_last in [false, true] {
        let mut draft = crate::ui::water_layers::tests::empty_root_created_via_button_for_tests();
        draft.water_dirty_layers.clear();
        draft.water_catalog_registration_required = false;
        draft.water_registration_source_hash = None;
        draft.terrain_documents.active_dirty = false;
        let mut object = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]))
            .manifest.terrain_objects_for_edit()[0].clone();
        object.id = TerrainId::parse("root-json-dirty-private").unwrap();
        draft.manifest.terrains = Some(Arc::from([object.clone()]));
        let mut session = crate::authoring::EditorAuthoringSession { draft: Some(draft), ..Default::default() };
        let mut editor = crate::resources::SceneEditorState::default();
        if delete_last {
            let draft = session.draft.as_mut().unwrap();
            draft.water.as_mut().unwrap().layers.clear();
            draft.water_masks.clear();
            session.mark_water_layers_changed(&mut editor);
        } else {
            session.draft.as_mut().unwrap().water.as_mut().unwrap().layers[0].water_level = 23.0;
            session.mark_water_definition_changed(&mut editor);
        }
        let draft = session.draft.as_mut().unwrap();
        assert!(draft.water_dirty_layers.is_empty(), "这两种JSON修改都不能依赖R8脏层兜底");
        let mut private = TerrainCanvas { identity: Some(TerrainDocumentIdentity {
            definition: Arc::new(object.clone()), manifest: object.source_manifest(&draft.manifest),
            folder: object.private_folder(&draft.scene_id),
        }), ..Default::default() };
        super::transitions::park(draft);
        private.swap(draft);
        assert!(session.has_scene_data_changes(), "根JSON停放后仍须阻止丢失未保存内容");
        let draft = session.draft.as_mut().unwrap();
        super::root_water::clear_saved(draft);
        assert!(!draft.stored_terrain_changes(), "同批保存成功才清除停放根水责任");
        super::root_water::restore_dirty(draft, std::iter::empty(), false);
        assert!(draft.stored_terrain_changes(), "没有R8的正式交接失败也必须允许显式重试");
        assert!(super::root_water::focus(draft));
        assert!(draft.terrain_documents.active_dirty, "切回根水同步恢复JSON脏位");
    }
}

#[test]
fn painting_after_save_survives_parking_and_cached_reopen_as_unsaved_work() {
    use bevy::prelude::*;
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    // Clipmap画布没有GLB读盘；仍走实际画布载入及saved清脏门。
    draft.manifest.terrain.kind = feathrise_scene_content::SceneTerrainKind::SparseClipmap;
    super::ensure_primary_canvas(&mut draft);
    let object = draft
        .terrain_documents
        .active
        .as_ref()
        .unwrap()
        .definition
        .clone();
    draft.manifest.terrain = feathrise_scene_content::SceneTerrainDescriptor::empty();
    draft.manifest.terrains = Some(Arc::from([(*object).clone()]));
    draft.puddle_mask = Some(Arc::new(vec![7; 16]));
    draft.puddles_dirty = false;
    draft.terrain_documents.saved.insert(object.id.clone()); // 已完成第一次保存。
    draft.puddle_mask = Some(Arc::new(vec![9; 16]));
    draft.puddles_dirty = true; // 保存后继续画A，再选择B会停放这个活动槽。
    super::transitions::park(&mut draft);
    assert!(draft.terrain_documents.dirty_stored.contains(&object.id));
    let cached = draft.terrain_documents.values[&object.id].clone();
    let identity = cached.identity.clone().unwrap();
    let reopened = bevy::tasks::block_on(super::load::load(
        identity,
        Some(cached),
        false,
        draft.terrain_documents.saved.contains(&object.id),
    ))
    .unwrap();
    assert!(reopened.puddles_dirty && reopened.dirty());
    assert_eq!(reopened.puddle_mask.as_deref().unwrap(), &vec![9; 16]);
}

fn history_fixture() -> (
    crate::authoring::EditorAuthoringSession,
    TerrainId,
    TerrainId,
) {
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    super::ensure_primary_canvas(&mut draft);
    let first = draft
        .terrain_documents
        .active
        .as_ref()
        .unwrap()
        .definition
        .clone();
    let mut second = (*first).clone();
    second.id = TerrainId::parse("canvas-b").unwrap();
    let second_id = second.id.clone();
    draft.manifest.terrain = feathrise_scene_content::SceneTerrainDescriptor::empty();
    draft.manifest.terrains = Some(Arc::from([(*first).clone(), second]));
    draft.water = Some(crate::authoring::water_asset_with_layer_for_tests(
        &draft.scene_id,
        &draft.folder,
    ));
    (
        crate::authoring::EditorAuthoringSession {
            draft: Some(draft),
            ..Default::default()
        },
        first.id.clone(),
        second_id,
    )
}

fn activate_history_canvas(draft: &mut crate::authoring::EditorSceneDraft, id: &TerrainId) {
    // 使用正式停放及作者槽交换，恢复真实缓存后才让统一撤销系统继续。
    super::transitions::park(draft);
    let mut canvas = if let Some(cached) = draft.terrain_documents.values.remove(id) {
        Arc::unwrap_or_clone(cached)
    } else {
        let object = draft
            .manifest
            .terrains
            .as_ref()
            .unwrap()
            .iter()
            .find(|object| &object.id == id)
            .unwrap()
            .clone();
        let mut water =
            crate::authoring::water_asset_with_layer_for_tests(&draft.scene_id, &draft.folder);
        water.layers[0].water_level = 900.0;
        TerrainCanvas {
            identity: Some(TerrainDocumentIdentity {
                manifest: object.source_manifest(&draft.manifest),
                folder: object.private_folder(&draft.scene_id),
                definition: Arc::new(object),
            }),
            water: Some(water),
            ..Default::default()
        }
    };
    draft.terrain_documents.dirty_stored.remove(id);
    canvas.swap(draft);
}

#[test]
fn saved_history_clear_scopes_same_frame_stroke_without_changing_canvas_pixels() {
    for background_retirement in [false, true] {
        let (mut session, first, second) = history_fixture();
        let pixels = Arc::new(vec![7; 16]);
        session.draft.as_mut().unwrap().puddle_mask = Some(pixels.clone());
        for _ in 0..7 {
            session.undo.push(EditorCommand::TerrainChanged(Vec::new()));
        }
        scope_history(&mut session);
        session.redo.push(session.undo[0].clone());
        assert_eq!(
            session
                .draft
                .as_ref()
                .unwrap()
                .terrain_documents
                .scoped_through,
            7
        );
        let mut retirement = crate::authoring::AuthoringRetirement::default();
        session.clear_history(background_retirement.then_some(&mut retirement));
        assert!(session.undo.is_empty() && session.redo.is_empty());
        let draft = session.draft.as_ref().unwrap();
        assert_eq!(draft.terrain_documents.scoped_through, 0);
        assert_eq!(
            draft
                .terrain_documents
                .active
                .as_ref()
                .unwrap()
                .definition
                .id,
            first
        );
        assert!(Arc::ptr_eq(draft.puddle_mask.as_ref().unwrap(), &pixels));

        // 保存完成与新笔收口在同一帧；Input尚未有机会再次夹短旧游标。
        session.undo.push(EditorCommand::SurfaceChanged {
            weight_changes: vec![crate::authoring::resources::ByteChange {
                index: 0,
                before: 0,
                after: 255,
            }],
            index_changes: Vec::new(),
        });
        scope_history(&mut session);
        activate_history_canvas(session.draft.as_mut().unwrap(), &second);
        scope_history(&mut session);
        assert!(
            matches!(&session.undo[0], EditorCommand::TerrainCanvas { id, command }
            if id == &first && matches!(command.as_ref(), EditorCommand::SurfaceChanged { .. }))
        );
    }
}

#[test]
fn bounded_water_prefix_trim_then_other_canvas_undo_redo_keeps_original_object_values() {
    use crate::authoring::*;
    use bevy::{asset::AssetPlugin, ecs::system::RunSystemOnce, prelude::*};
    let (mut session, first, second) = history_fixture();
    let map = session.draft.as_ref().unwrap().scene_id.clone();
    let layer_id = session
        .draft
        .as_ref()
        .unwrap()
        .water
        .as_ref()
        .unwrap()
        .layers[0]
        .id
        .clone();
    let collection =
        feathrise_scene_content::SceneTerrainCollection::capture(&session.draft.as_ref().unwrap().manifest);
    session.undo.push(EditorCommand::TerrainObjectsChanged {
        before: collection.clone(),
        after: collection,
    });
    // 使用正式256条水历史预算，最旧水命令之前的场景命令也必须一并裁掉。
    for index in 0..256 {
        session
            .draft
            .as_mut()
            .unwrap()
            .water
            .as_mut()
            .unwrap()
            .layers[0]
            .water_level = index as f32 + 1.0;
        session.push_bounded_water_history(EditorCommand::WaterLevelChanged {
            layer_id: layer_id.clone(),
            before: index as f32,
            after: index as f32 + 1.0,
        });
        scope_history(&mut session);
    }
    assert_eq!(
        session
            .draft
            .as_ref()
            .unwrap()
            .terrain_documents
            .scoped_through,
        257
    );
    session
        .draft
        .as_mut()
        .unwrap()
        .water
        .as_mut()
        .unwrap()
        .layers[0]
        .water_level = 257.0;
    session.push_bounded_water_history(EditorCommand::WaterLevelChanged {
        layer_id,
        before: 256.0,
        after: 257.0,
    });
    assert_eq!(session.undo.len(), 256);
    assert_eq!(
        session
            .draft
            .as_ref()
            .unwrap()
            .terrain_documents
            .scoped_through,
        255
    );
    scope_history(&mut session);
    assert!(
        matches!(session.undo.last(), Some(EditorCommand::TerrainCanvas { id, .. }) if id == &first)
    );
    activate_history_canvas(session.draft.as_mut().unwrap(), &second);

    let mut selection = crate::ui::terrain_objects::TerrainObjectsEditorState::default();
    selection.scene_id = Some(map.clone());
    selection.selected = Some(second.clone());
    selection.selected_index = Some(1);
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .insert_resource(session)
        .insert_resource(crate::resources::SceneEditorState {
            open: true,
            selected_scene_id: Some(map.clone()),
            undo_requested: true,
            ..default()
        })
        .insert_resource(selection)
        .insert_resource(EditorPreviewAssets {
            portal_base_mesh: default(),
            portal_arrow_head_mesh: default(),
            portal_arrow_shaft_mesh: default(),
            portal_edge_beam_mesh: default(),
            portal_edge_ring_mesh: default(),
            portal_base_material: default(),
            portal_arrow_material: default(),
            portal_edge_beam_material: default(),
            portal_edge_ring_material: default(),
            portal_label_font: default(),
        })
        .init_resource::<TerrainDocumentTask>()
        .init_resource::<courtyards::CourtyardAuthoringState>()
        .init_resource::<GlobalEnvironmentAuthoringState>()
        .init_resource::<WaterMaskMaterializeTask>()
        .init_resource::<crate::authoring::surface_layer_edit::SurfaceLayerEditTask>()
        .init_resource::<SurfaceBrushStroke>()
        .init_resource::<WaterBrushStroke>()
        .init_resource::<crate::ui::placement_water::PlacementInstanceIndex>()
        .init_resource::<WaterFileHandoff>();
    app.world_mut().run_system_once(apply_undo_redo).unwrap();
    assert_eq!(
        app.world()
            .resource::<TerrainDocumentTask>()
            .requested
            .as_ref(),
        Some(&first)
    );
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.undo.len(), 256);
    assert!(session.redo.is_empty());
    let draft = session.draft.as_ref().unwrap();
    assert_eq!(draft.water.as_ref().unwrap().layers[0].water_level, 900.0);
    assert_eq!(
        draft.terrain_documents.values[&first]
            .water
            .as_ref()
            .unwrap()
            .layers[0]
            .water_level,
        257.0
    );

    activate_history_canvas(
        app.world_mut()
            .resource_mut::<EditorAuthoringSession>()
            .draft
            .as_mut()
            .unwrap(),
        &first,
    );
    app.world_mut()
        .resource_mut::<crate::ui::terrain_objects::TerrainObjectsEditorState>()
        .documents_loading = false;
    app.world_mut().run_system_once(apply_undo_redo).unwrap();
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(
        session
            .draft
            .as_ref()
            .unwrap()
            .water
            .as_ref()
            .unwrap()
            .layers[0]
            .water_level,
        256.0
    );
    assert!(
        matches!(session.redo.last(), Some(EditorCommand::TerrainCanvas { id, .. }) if id == &first)
    );
    assert_eq!(
        session.draft.as_ref().unwrap().terrain_documents.values[&second]
            .water
            .as_ref()
            .unwrap()
            .layers[0]
            .water_level,
        900.0
    );
    app.world_mut()
        .resource_mut::<crate::resources::SceneEditorState>()
        .redo_requested = true;
    app.world_mut().run_system_once(apply_undo_redo).unwrap();
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(
        session
            .draft
            .as_ref()
            .unwrap()
            .water
            .as_ref()
            .unwrap()
            .layers[0]
            .water_level,
        257.0
    );
    assert!(
        matches!(session.undo.last(), Some(EditorCommand::TerrainCanvas { id, .. }) if id == &first)
    );
    assert_eq!(
        session.draft.as_ref().unwrap().terrain_documents.values[&second]
            .water
            .as_ref()
            .unwrap()
            .layers[0]
            .water_level,
        900.0
    );
}
