//! 实际后台预览冻结对象与源范围，同图换画布拒绝旧GPU结果而保留原作者字节。

use super::*;
use crate::authoring::terrain::tests::test_heightfield_draft;
use bevy::{asset::AssetPlugin, ecs::system::RunSystemOnce};
use feathrise_scene_content::{
    SceneSurfaceAsset, SceneSurfaceLayer, ResolvedSurfacePresetCatalog, SURFACE_PRESET_SCHEMA_VERSION,
    SurfacePresetCatalogAsset, SurfacePresetDescriptor,
};

pub(super) fn draft() -> crate::authoring::EditorSceneDraft {
    let mut draft = test_heightfield_draft(Arc::new(vec![0; 121]));
    let mut object = draft.manifest.terrain_objects_for_edit()[0].clone();
    object.id = TerrainId::parse("paint-a").unwrap();
    object.terrain.kind = SceneTerrainKind::SparseClipmap;
    object.world_min = [-10.0, -20.0];
    object.world_max = [30.0, 40.0];
    draft.manifest.world_min = [-500.0; 2];
    draft.manifest.world_max = [500.0; 2];
    draft.manifest.terrain = feathrise_scene_content::SceneTerrainDescriptor::empty();
    draft.manifest.terrains = Some(vec![object.clone()].into());
    let manifest = object.source_manifest(&draft.manifest);
    draft.terrain_documents.active = Some(
        crate::authoring::terrain_documents::TerrainDocumentIdentity {
            definition: Arc::new(object),
            manifest,
            folder: "scenes/EDGE-TEST/terrains/paint-a".into(),
        },
    );
    draft.surface = Some(SceneSurfaceAsset {
        schema_version: feathrise_scene_content::SCENE_SURFACE_SCHEMA_VERSION,
        scene_id: draft.scene_id.clone(),
        resolution: [2; 2],
        layers: Vec::new(),
        weight_source_path: "scenes/EDGE-TEST/weights.rgba8".into(),
        weight_texture_path: "scenes/EDGE-TEST/weights.ktx2".into(),
        index_source_path: "scenes/EDGE-TEST/indices.rgba32".into(),
        index_texture_path: "scenes/EDGE-TEST/indices.ktx2".into(),
    });
    draft.surface_indices = Some(Arc::new(vec![0; 64]));
    draft.surface_weights = Some(Arc::new(vec![0; 16]));
    draft.surface_dirty = true;
    draft
}

fn presets() -> SurfacePresetRegistry {
    let asset = SurfacePresetCatalogAsset {
        schema_version: SURFACE_PRESET_SCHEMA_VERSION,
        texture_resolution: [2; 2],
        base_color_array_path: "textures/paint/base.ktx2".into(),
        normal_array_path: "textures/paint/normal.ktx2".into(),
        arm_height_array_path: "textures/paint/arm.ktx2".into(),
        presets: vec![SurfacePresetDescriptor {
            id: "rammed_earth".into(),
            aliases: Vec::new(),
            label: "夯土".into(),
            base_color_srgba: [0.7, 0.5, 0.3, 1.0],
            base_color_texture_path: None,
            normal_texture_path: None,
            arm_height_texture_path: None,
            tile_meters: 2.0,
            normal_strength: 1.0,
            normal_y_flip: false,
            height_blend_strength: 0.0,
            perceptual_roughness: 0.9,
            metallic: 0.0,
            crater_response: 0.1,
            projection: Default::default(),
        }],
    };
    SurfacePresetRegistry {
        catalog: Handle::default(),
        revision: 1,
        resolved: ResolvedSurfacePresetCatalog::build(&asset).unwrap(),
        last_error: None,
    }
}

fn app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_resource::<SurfacePreviewState>()
        .init_resource::<SurfaceBrushStroke>()
        .insert_resource(presets())
        .insert_resource(SceneEditorState {
            open: true,
            ..default()
        })
        .insert_resource(EditorAuthoringSession {
            draft: Some(draft()),
            surface_revision: 3,
            ..default()
        });
    app
}

#[test]
fn surface_cpu_preview_uses_object_bounds_and_preserves_empty_to_first_painted_layer() {
    let mut app = app();
    for layer_count in [0, 1] {
        if layer_count == 1 {
            let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
            session.surface_revision += 1;
            let draft = session.draft.as_mut().unwrap();
            draft
                .surface
                .as_mut()
                .unwrap()
                .layers
                .push(SceneSurfaceLayer {
                    id: "earth-layer".into(),
                    label: "夯土层".into(),
                    preset_id: "rammed_earth".into(),
                });
            Arc::get_mut(draft.surface_weights.as_mut().unwrap()).unwrap()[0] = 255;
        }
        let weights = app
            .world()
            .resource::<EditorAuthoringSession>()
            .draft
            .as_ref()
            .unwrap()
            .surface_weights
            .clone()
            .unwrap();
        app.world_mut()
            .run_system_once(start_surface_preview_rebuild)
            .unwrap();
        let mut task = app
            .world_mut()
            .resource_mut::<SurfacePreviewState>()
            .task
            .take()
            .unwrap();
        let started = Instant::now();
        let (map, terrain, revision, preset, result) = loop {
            if let Some(result) = check_ready(&mut task) {
                break result;
            }
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "四像素正式Mip编码必须完成"
            );
            std::thread::yield_now();
        };
        let result = result.unwrap();
        assert_eq!(terrain, Some(TerrainId::parse("paint-a").unwrap()));
        assert_eq!(
            map,
            app.world()
                .resource::<EditorAuthoringSession>()
                .draft
                .as_ref()
                .unwrap()
                .scene_id
        );
        assert_eq!(revision, 3 + u64::from(layer_count));
        assert_eq!(preset, 1);
        assert_eq!(result.world_min, Vec2::new(-10.0, -20.0));
        assert_eq!(result.world_max, Vec2::new(30.0, 40.0));
        assert_eq!(result.layer_count, layer_count);
        assert!((result.crater_response.as_ref().unwrap().sample(Vec2::new(0.0, -5.0))
            - if layer_count == 0 { 1.0 } else { 0.1 }).abs() < 0.001,
            "真实预览worker必须按相同作者槽/像素和当前预设0.1构建CPU响应，不能读取旧文件或默认土壤");
        assert_eq!(
            &result.weights.data.as_ref().unwrap()[..16],
            weights.as_slice()
        );
        assert!(Arc::ptr_eq(
            &weights,
            app.world()
                .resource::<EditorAuthoringSession>()
                .draft
                .as_ref()
                .unwrap()
                .surface_weights
                .as_ref()
                .unwrap()
        ));
    }
}

#[test]
fn surface_ready_candidate_from_another_canvas_is_rejected_before_asset_adoption() {
    let mut app = app();
    app.init_resource::<Assets<Image>>()
        .init_resource::<Assets<ShaderBuffer>>()
        .init_resource::<Assets<TerrainSurfaceMaterial>>()
        .init_resource::<TerrainSurfaceFallbackImages>();
    let map = app
        .world()
        .resource::<EditorAuthoringSession>()
        .draft
        .as_ref()
        .unwrap()
        .scene_id
        .clone();
    app.world_mut()
        .resource_mut::<SurfacePreviewState>()
        .candidate = Some(SurfacePreviewCandidate {
        crater_response: None,
        scene_id: map,
        terrain_id: Some(TerrainId::parse("paint-b").unwrap()),
        revision: 3,
        preset_revision: 1,
        material: Handle::default(),
        images: Vec::new(),
        layer_table: Handle::default(),
        ready: Arc::new(AtomicBool::new(true)),
        failed: Arc::new(AtomicBool::new(false)),
        started: Instant::now(),
    });
    app.world_mut()
        .run_system_once(poll_surface_preview_rebuild)
        .unwrap();
    let state = app.world().resource::<SurfacePreviewState>();
    assert!(state.candidate.is_none());
    assert!(state.applied_scene_id.is_none() && state.material.is_none());
    assert!(
        state.failed_identity.is_none(),
        "迟到对象不是当前画布构建失败"
    );
}

#[test]
fn surface_candidate_publishes_crater_response_and_material_as_one_complete_revision() {
    let mut app = app();
    app.init_resource::<Assets<Image>>()
        .init_resource::<Assets<ShaderBuffer>>()
        .init_resource::<Assets<TerrainSurfaceMaterial>>()
        .init_resource::<TerrainSurfaceFallbackImages>();
    let map = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().scene_id.clone();
    let response = |coefficient| Arc::new(SurfaceCraterResponse::from_author_buffers(
        Vec2::splat(-10.0), Vec2::splat(10.0), [1, 1], Arc::new(vec![0; 16]),
        Arc::new(vec![255, 0, 0, 0]), vec![coefficient]).unwrap());
    let old = response(1.0);
    let next = response(0.1);
    let ready = Arc::new(AtomicBool::new(false));
    {
        let mut state = app.world_mut().resource_mut::<SurfacePreviewState>();
        state.applied_scene_id = Some(map.clone());
        state.applied_terrain_id = Some(TerrainId::parse("paint-a").unwrap());
        state.applied_revision = 2;
        state.material = Some(Handle::default());
        state.crater_response = Some(old.clone());
        state.candidate = Some(SurfacePreviewCandidate { scene_id: map, terrain_id: state.applied_terrain_id.clone(),
            revision: 3, preset_revision: 1, material: Handle::default(), crater_response: Some(next.clone()),
            images: Vec::new(), layer_table: Handle::default(), ready: ready.clone(),
            failed: Arc::new(AtomicBool::new(false)), started: Instant::now() });
    }
    app.world_mut().run_system_once(poll_surface_preview_rebuild).unwrap();
    assert!(Arc::ptr_eq(app.world().resource::<SurfacePreviewState>().crater_response.as_ref().unwrap(), &old));
    ready.store(true, Ordering::Release);
    app.world_mut().run_system_once(poll_surface_preview_rebuild).unwrap();
    let state = app.world().resource::<SurfacePreviewState>();
    assert_eq!(state.applied_revision, 3);
    assert!(Arc::ptr_eq(state.crater_response.as_ref().unwrap(), &next));
    assert!((state.crater_response.as_ref().unwrap().sample(Vec2::ZERO) - 0.1).abs() < 0.001);
}
