//! 真实单击经过最近网格、完整画布、统一候选卡片和属性应用，覆盖平移旋转后的多网格。
use feathrise_editor_ui::UiInteraction;
#[path = "root_water_selection_tests.rs"]
mod root_water_selection_tests;
use super::*;
use crate::{
    authoring::{
        WaterBrushStroke, WaterNetworkEditState,
        terrain_documents::{TerrainCanvas, TerrainDocumentIdentity},
    },
    components::{SceneEditorCamera, SceneEditorViewport},
    resources::SceneEditorViewportState,
    types::EditorTool,
    ui::courtyard::CourtyardUiState,
    viewport::content_selection::{
        layer_ui::{
            CandidateAction, LayerCandidateSlot, layer_candidate_interactions,
            sync_layer_candidate_ui,
        },
        select_editor_viewport_content,
    },
};
use bevy::{
    asset::AssetPlugin,
    camera::{ComputedCameraValues, RenderTargetInfo},
    ui::RelativeCursorPosition,
};
use feathrise_scene_content::*;

fn canvas(
    draft: &crate::authoring::EditorSceneDraft,
    object: &SceneTerrainObject,
    suffix: &str,
) -> TerrainCanvas {
    let folder = object.private_folder(&draft.scene_id);
    let author = |name: &str, enabled: bool| SceneVegetationAuthorLayer {
        id: format!("{name}-{suffix}"),
        label: name.into(),
        group_id: "ground".into(),
        output_layer_id: "grass".into(),
        density_source_path: format!("{folder}/{name}.r8"),
        resolution: [2, 2],
        enabled,
        locked: name == "grass-b",
        opacity: 1.0,
        priority: 0,
        weight: 1.0,
    };
    let authors = vec![
        author("grass-a", true),
        author("grass-b", true),
        author("hidden", false),
        author("empty", true),
    ];
    let vegetation = SceneVegetationAsset {
        schema_version: feathrise_scene_content::SCENE_SCHEMA_VERSION,
        scene_id: draft.scene_id.clone(),
        layers: vec![SceneVegetationLayer {
            id: "grass".into(),
            label: "草".into(),
            kind: SceneVegetationKind::Blade,
            runtime_profile: Some("hn01_ordinary_grass".into()),
            runtime_layer_id: Some(1),
            prototype_id: None,
            asset_path: "grass.glb".into(),
            density_source_path: format!("{folder}/grass.r8"),
            density_texture_path: format!("{folder}/grass.ktx2"),
            resolution: [2, 2],
            spacing_meters: 1.0,
            scale_range: [1.0, 1.0],
            max_instances: u32::MAX,
            seed: 1,
        }],
        authoring: Some(SceneVegetationAuthoring {
            groups: vec![SceneVegetationAuthorGroup {
                id: "ground".into(),
                label: "草".into(),
                mode: SceneVegetationAuthorGroupMode::Additive,
                output_merge: SceneVegetationOutputMergeMode::Maximum,
                capacity: 1.0,
                claim_threshold: 0.0,
                selection_seed: 1,
                selection_cell_meters: 1.0,
                enabled: true,
            }],
            layers: authors,
        }),
    };
    TerrainCanvas {
        identity: Some(TerrainDocumentIdentity {
            definition: Arc::new(object.clone()),
            manifest: object.source_manifest(&draft.manifest),
            folder: folder.clone(),
        }),
        surface: Some(SceneSurfaceAsset {
            schema_version: feathrise_scene_content::SCENE_SURFACE_SCHEMA_VERSION,
            scene_id: draft.scene_id.clone(),
            weight_source_path: format!("{folder}/weights.rgba8"),
            weight_texture_path: format!("{folder}/weights.ktx2"),
            index_source_path: format!("{folder}/indices.rgba32"),
            index_texture_path: format!("{folder}/indices.ktx2"),
            resolution: [2, 2],
            layers: ["soil-a", "soil-b"]
                .into_iter()
                .map(|name| SceneSurfaceLayer {
                    id: format!("{name}-{suffix}"),
                    label: name.into(),
                    preset_id: "soil".into(),
                })
                .collect(),
        }),
        surface_weights: Some(Arc::new([128, 127, 0, 0].repeat(4))),
        surface_indices: Some(Arc::new(
            [0_u32, 1, 0, 0]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>()
                .repeat(4),
        )),
        vegetation: Some(vegetation),
        vegetation_author_densities: ["grass-a", "grass-b", "hidden", "empty"]
            .into_iter()
            .map(|name| {
                (
                    format!("{name}-{suffix}"),
                    Arc::new(vec![if name == "empty" { 0 } else { 255 }; 4]),
                )
            })
            .collect(),
        ..default()
    }
}

fn fixture() -> (App, Entity) {
    let (mut app, panel) = app();
    app.add_plugins(AssetPlugin {
        file_path: concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets").into(),
        watch_for_changes_override: Some(false),
        ..default()
    })
    .init_asset::<Font>()
    .init_asset::<Mesh>()
    .init_resource::<ButtonInput<KeyCode>>()
    .init_resource::<CourtyardUiState>()
    .init_resource::<WaterBrushStroke>()
    .init_resource::<WaterNetworkEditState>()
    .init_resource::<feathrise_scene::DynamicClipmapGroundSurface>()
    .configure_sets(
        Update,
        (
            SceneEditorSystemSet::Input,
            SceneEditorSystemSet::Tasks,
            SceneEditorSystemSet::Presentation,
        )
            .chain(),
    )
    .add_systems(
        Update,
        (layer_candidate_interactions, select_editor_viewport_content)
            .chain()
            .in_set(SceneEditorSystemSet::Input),
    )
    .add_systems(
        Update,
        sync_layer_candidate_ui.in_set(SceneEditorSystemSet::Presentation),
    );
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        let mut objects = draft.manifest.terrains.clone().unwrap().to_vec();
        for (index, object) in objects.iter_mut().enumerate() {
            object.world_min = [-32.0; 2];
            object.world_max = [32.0; 2];
            object.terrain.resolution = [257; 2];
            object.transform.translation = [index as f32 * 120.0, 0.0, index as f32 * 40.0];
            object.transform.yaw_degrees = index as f32 * 90.0;
            object.transform.scale = [1.5, 1.0, 2.0];
        }
        draft.manifest.terrains = Some(objects.clone().into());
        for (index, object) in objects.iter().enumerate() {
            draft.terrain_documents.values.insert(
                object.id.clone(),
                Arc::new(canvas(draft, object, if index == 0 { "a" } else { "b" })),
            );
        }
        draft.terrain_ground = Some(TerrainGroundSnapshot::test_meshes(
            draft.scene_id.clone(),
            &objects,
            avian3d::prelude::Collider::cuboid(64.0, 2.0, 64.0),
        ));
    }
    let camera = app
        .world_mut()
        .spawn((
            SceneEditorCamera,
            Camera {
                computed: ComputedCameraValues {
                    target_info: Some(RenderTargetInfo {
                        physical_size: UVec2::splat(128),
                        scale_factor: 1.0,
                    }),
                    clip_from_view: bevy::math::proj::perspective_infinite_reverse(
                        50_f32.to_radians(),
                        1.0,
                        0.1,
                    ),
                    ..default()
                },
                ..default()
            },
            Transform::default(),
        ))
        .id();
    app.world_mut().spawn((
        SceneEditorViewport,
        RelativeCursorPosition {
            cursor_over: true,
            normalized: Some(Vec2::ZERO),
        },
    ));
    app.world_mut().spawn((
        LayerCandidateSlot,
        Node {
            display: Display::None,
            ..default()
        },
    ));
    app.insert_resource(SceneEditorViewportState {
        _image: Handle::default(),
        camera,
        center: Vec3::ZERO,
        scene_center: Vec3::ZERO,
        scene_half_extents: Vec3::splat(200.0),
        extent: 20.0,
        yaw: 0.0,
        pitch: std::f32::consts::FRAC_PI_2,
        framed_scene_id: None,
        render_size: UVec2::splat(128),
    });
    app.world_mut().resource_mut::<SceneEditorState>().tool = EditorTool::Inspect;
    (app, panel)
}

fn click_grid(app: &mut App, index: usize) {
    let session = app.world().resource::<EditorAuthoringSession>();
    let object = &session
        .draft
        .as_ref()
        .unwrap()
        .manifest
        .terrains
        .as_ref()
        .unwrap()[index];
    let center = Vec3::from_array(object.transform.translation);
    let camera = app.world().resource::<SceneEditorViewportState>().camera;
    *app.world_mut().get_mut::<Transform>(camera).unwrap() =
        Transform::from_translation(center + Vec3::Y * 20.0).looking_at(center, Vec3::Z);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.update(); // 正式鼠标按下系统生成一次请求，不直接写入候选或正式选择。
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .reset_all();
}
fn finish_layers(app: &mut App, suffix: &str) {
    let expected = LayerTarget::Vegetation(format!("grass-a-{suffix}"));
    let started = Instant::now();
    loop {
        app.update();
        let state = app.world().resource::<LayerCandidateState>();
        if state.selected.as_ref() == Some(&expected) && state.candidates.len() == 5 {
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "单击后的完整画布候选未采用：selected={:?}, pending={}, wait={}",
            state.selected,
            state.task.is_some(),
            state.waiting_canvas.is_some()
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn choose(app: &mut App, expected: LayerTarget) {
    let index = app
        .world()
        .resource::<LayerCandidateState>()
        .candidates
        .iter()
        .position(|candidate| candidate.target == expected)
        .or_else(|| app.world().resource::<LayerCandidateState>().candidates.iter().position(|candidate|
            matches!((&candidate.target, &expected), (LayerTarget::RootWater(kind, id), LayerTarget::Water(other_kind, other_id))
                if kind == other_kind && id == other_id)))
        .unwrap();
    let world = app.world_mut();
    let row = world
        .query::<(Entity, &CandidateAction)>()
        .iter(world)
        .find(|(_, action)| matches!(action, CandidateAction::Select(at) if *at == index))
        .unwrap()
        .0;
    app.world_mut().entity_mut(row).insert((UiInteraction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
    app.update();
    assert_eq!(
        app.world()
            .resource::<LayerCandidateState>()
            .selected
            .as_ref(),
        Some(&expected)
    );
    *app.world_mut().get_mut::<UiInteraction>(row).unwrap() = UiInteraction::None;
    app.update();
}

#[test]
fn layered_canvas_single_click_lists_and_switches_grass_surface_and_mesh_across_transformed_grids()
{
    let (mut app, _) = fixture();
    for (index, suffix) in [(0, "a"), (1, "b"), (0, "a")] {
        let confirmations = app
            .world()
            .resource::<LayerCandidateState>()
            .selection_revision;
        click_grid(&mut app, index);
        finish_layers(&mut app, suffix);
        assert_eq!(
            app.world()
                .resource::<LayerCandidateState>()
                .selection_revision,
            confirmations + 1,
            "载入后继续原点击只确认最上层一次，不先确认网格再伪造第二次选择"
        );
        let world = app.world_mut();
        assert_eq!(
            world
                .query_filtered::<&Node, With<LayerCandidateSlot>>()
                .single(world)
                .unwrap()
                .display,
            Display::Flex
        );
        assert_eq!(
            world
                .query::<&CandidateAction>()
                .iter(world)
                .filter(|action| matches!(action, CandidateAction::Select(_)))
                .count(),
            5
        );
        choose(
            &mut app,
            LayerTarget::Vegetation(format!("grass-b-{suffix}")),
        );
        assert_eq!(
            app.world()
                .resource::<SceneEditorState>()
                .selected_vegetation_layer,
            1
        );
        choose(&mut app, LayerTarget::Surface(format!("soil-b-{suffix}")));
        assert_eq!(
            app.world()
                .resource::<SceneEditorState>()
                .selected_surface_layer,
            1
        );
        let id = app
            .world()
            .resource::<EditorAuthoringSession>()
            .draft
            .as_ref()
            .unwrap()
            .manifest
            .terrains
            .as_ref()
            .unwrap()[index]
            .id
            .clone();
        choose(&mut app, LayerTarget::TerrainObject(id.as_str().into()));
        assert_eq!(
            app.world()
                .resource::<TerrainObjectsEditorState>()
                .selected
                .as_ref(),
            Some(&id)
        );
        assert_eq!(
            app.world()
                .resource::<LayerCandidateState>()
                .candidates
                .len(),
            5
        );
        assert_eq!(
            app.world().resource::<SceneEditorState>().tool,
            EditorTool::Inspect
        );
    }
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    );
}

#[test]
fn layered_canvas_old_wait_is_cancelled_by_new_grid_click_and_late_result_cannot_steal_tree_selection()
 {
    let (mut app, _) = fixture();
    click_grid(&mut app, 0);
    let started = Instant::now();
    while app
        .world()
        .resource::<LayerCandidateState>()
        .waiting_canvas
        .is_none()
    {
        app.update();
        assert!(started.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(1));
    }
    let held = app.world_mut().resource_mut::<terrain_documents::TerrainDocumentTask>()
        .pause_pending_for_tests();
    let confirmations = app.world().resource::<LayerCandidateState>().selection_revision;
    // 第一个画布的载入还未完成就点击另一个网格；旧载入只可转接最新票据，不能抢回第一个网格。
    click_grid(&mut app, 1);
    let second = TerrainId::parse("terrain-b").unwrap();
    while !app.world().resource::<LayerCandidateState>().waiting_canvas.as_ref()
        .is_some_and(|wait| wait.request.hit_terrain.as_ref().is_some_and(|(id, index)| id == &second && *index == 1)) {
        app.update();
        assert!(started.elapsed() < Duration::from_secs(10), "B的真实射线必须在A的载入仍在途时完成");
        std::thread::sleep(Duration::from_millis(1));
    }
    for _ in 0..2 { app.update(); }
    assert!(!app.world().resource::<LayerCandidateState>().waiting_canvas.as_ref().unwrap().observed_loading,
        "A仍占用画布槽时，B不能把全局loading当成自己的加载已经开始");
    app.world_mut().resource_mut::<terrain_documents::TerrainDocumentTask>()
        .resume_pending_for_tests(held);
    finish_layers(&mut app, "b");
    assert_eq!(app.world().resource::<LayerCandidateState>().selection_revision, confirmations + 1,
        "A完成及退役只归还槽位；B的一次真实点击必须最终仅确认一次最上层草和完整5候选");
    click_grid(&mut app, 0);
    let chosen = LayerTarget::Placement("explicit-tree-selection".into());
    {
        let mut state = app.world_mut().resource_mut::<LayerCandidateState>();
        state.clear_for_scene_tree();
        state.selected = Some(chosen.clone());
    }
    for _ in 0..50 {
        app.update();
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(
        app.world()
            .resource::<LayerCandidateState>()
            .selected
            .as_ref(),
        Some(&chosen)
    );
    assert!(
        app.world()
            .resource::<LayerCandidateState>()
            .waiting_canvas
            .is_none()
    );
}

#[test]
fn layered_canvas_scene_and_author_changes_reject_waiting_and_late_candidate_results() {
    for change_scene in [false, true] {
        let (mut app, _) = fixture();
        click_grid(&mut app, 0);
        let started = Instant::now();
        while app
            .world()
            .resource::<LayerCandidateState>()
            .waiting_canvas
            .is_none()
        {
            app.update();
            assert!(started.elapsed() < Duration::from_secs(10));
            std::thread::sleep(Duration::from_millis(1));
        }
        {
            let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
            session.revision += 1;
            if change_scene {
                session.draft.as_mut().unwrap().scene_id = SceneId::parse("MAP-other-layered").unwrap();
            }
        }
        if change_scene {
            app.world_mut()
                .resource_mut::<SceneEditorState>()
                .selected_scene_id = Some(SceneId::parse("MAP-other-layered").unwrap());
        }
        for _ in 0..50 {
            app.update();
            std::thread::sleep(Duration::from_millis(1));
        }
        let state = app.world().resource::<LayerCandidateState>();
        assert!(
            state.waiting_canvas.is_none()
                && state.candidates.is_empty()
                && state.selected.is_none()
        );
    }
}
