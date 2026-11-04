//! 真实只读捕获与后台编码回归；验证旧来源拒收、保存/历史不变及正式GLB/纹理字节。

use super::*;
use crate::authoring::{EditorAuthoringSession, test_heightfield_draft};
use bevy::tasks::AsyncComputeTaskPool;
use feathrise_map_content::{embedded_snapshot::SceneSnapshot, *};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

fn app() -> App {
    let mut app = App::new();
    let mut draft = test_heightfield_draft(Arc::new(vec![0; 121]));
    draft.placements.schema_version = MAP_SCHEMA_VERSION;
    draft.portals.schema_version = MAP_SCHEMA_VERSION;
    app.add_plugins(MinimalPlugins)
        .insert_resource(EditorAuthoringSession {
            draft: Some(draft),
            revision: 3,
            ..default()
        })
        .add_plugins(SnapshotCapturePlugin);
    app
}

fn placement(id: usize) -> MapPlacementRecord {
    serde_json::from_value(serde_json::json!({
        "id":format!("placement-{id}"), "asset_path":"", "asset_target": {
            "kind":"cuboid", "size_mm":[2000,2000,2000], "color_srgb":[160,160,160], "roughness_milli":700, "collision":true
        }, "translation":[2.0,0.0,3.0], "yaw_degrees":0.0, "scale":[1.0,1.0,1.0]
    })).unwrap()
}

fn settle(app: &mut App) {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        app.update();
        let state = app.world().resource::<EmbeddedSnapshotState>();
        if !state.pending && (state.ready.is_some() || state.error.is_some()) {
            return;
        }
        assert!(Instant::now() < until, "后台快照在测试期限内未完成");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn modal_fixture() -> App {
    let mut app = app();
    app.insert_resource(crate::resources::MapEditorState { open: true, ..default() })
        .init_resource::<crate::resources::MapEditorSettingsState>()
        .init_resource::<crate::resources::MapEditorNumericScrubMouseGuard>()
        .init_resource::<feathrise_editor_core::EditorHostOverlay>()
        .init_resource::<feathrise_editor_ui::ActiveNumericInputScrub>()
        .init_resource::<ButtonInput<MouseButton>>()
        .add_systems(Update,
            crate::ui::numeric_scrub_mouse_guard::update_map_editor_numeric_scrub_mouse_guard
                .before(systems::advance));
    app.world_mut().resource_mut::<EmbeddedSnapshotState>().enabled = true;
    app
}

fn set_modal(app: &mut App, settings: bool, open: bool) {
    if settings {
        app.world_mut().resource_mut::<crate::resources::MapEditorSettingsState>().open = open;
    } else {
        // 宿主日志与退出确认共享此原输入门；不在快照层伪造各自的UI或生命周期。
        app.world_mut().resource_mut::<feathrise_editor_core::EditorHostOverlay>().active = open;
    }
}

#[test]
fn modal_input_ownership_preserves_completed_snapshot_through_the_release_frame() {
    for settings in [false, true] {
        let mut app = modal_fixture();
        settle(&mut app);
        let ready = app.world().resource::<EmbeddedSnapshotState>().ready.as_ref().unwrap().clone();
        set_modal(&mut app, settings, true);
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        for _ in 0..4 {
            app.update();
            assert!(app.world().resource::<crate::resources::MapEditorNumericScrubMouseGuard>().blocked());
            let state = app.world().resource::<EmbeddedSnapshotState>();
            assert!(!state.pending && state.capture.is_none() && state.task.is_none());
            assert!(Arc::ptr_eq(state.ready.as_ref().unwrap(), &ready));
            assert_eq!(state.revision, ready.author_revision);
            assert!(ready_is_current(app.world()));
            assert_eq!(waiting_reason(app.world()), None);
        }
        set_modal(&mut app, settings, false);
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
        app.update();
        assert!(app.world().resource::<crate::resources::MapEditorNumericScrubMouseGuard>().blocked(),
            "快照已完整也不能提前归还关闭弹窗的release输入");
        assert!(ready_is_current(app.world()));
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
        app.update();
        assert!(!app.world().resource::<crate::resources::MapEditorNumericScrubMouseGuard>().blocked());
        assert!(Arc::ptr_eq(app.world().resource::<EmbeddedSnapshotState>().ready.as_ref().unwrap(), &ready));
    }
}

#[test]
fn modal_input_ownership_allows_initial_capture_and_requires_the_new_author_revision() {
    for settings in [false, true] {
        let mut app = modal_fixture();
        app.world_mut().resource_mut::<EditorAuthoringSession>().draft.as_mut().unwrap()
            .placements.placements = vec![placement(0)];
        set_modal(&mut app, settings, true);
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        settle(&mut app);
        assert!(app.world().resource::<crate::resources::MapEditorNumericScrubMouseGuard>().blocked());
        assert!(ready_is_current(app.world()), "遮挡不应饿死工程视口首份完整来源");
        let previous = app.world().resource::<EmbeddedSnapshotState>().ready.as_ref().unwrap().clone();
        {
            let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
            session.revision += 1;
            session.draft.as_mut().unwrap().placements.placements[0].translation[0] = 7.0;
        }
        assert!(!ready_is_current(app.world()), "打开浮层不授予旧作者修订启动信用");
        app.update();
        assert!(app.world().resource::<EmbeddedSnapshotState>().pending);
        settle(&mut app);
        let state = app.world().resource::<EmbeddedSnapshotState>();
        let ready = state.ready.as_ref().unwrap();
        assert!(ready.author_revision > previous.author_revision);
        let snapshot: SceneSnapshot = serde_json::from_slice(&ready.bytes).unwrap();
        assert_eq!(snapshot.placements.placements[0].translation[0], 7.0);
        assert!(ready_is_current(app.world()));
        assert!(app.world().resource::<crate::resources::MapEditorNumericScrubMouseGuard>().blocked());
        let session = app.world().resource::<EditorAuthoringSession>();
        assert_eq!(session.successful_save_sequence, 0);
        assert!(session.undo.is_empty() && session.redo.is_empty());
    }
}

#[test]
fn real_author_transactions_still_block_even_when_the_completed_revision_is_unchanged() {
    let mut app = modal_fixture();
    settle(&mut app);
    let ready = app.world().resource::<EmbeddedSnapshotState>().ready.as_ref().unwrap().clone();
    set_modal(&mut app, false, true);
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
    let input = app.world_mut().spawn_empty().id();
    app.world_mut().resource_mut::<feathrise_editor_ui::ActiveNumericInputScrub>().entity = Some(input);
    app.update();
    assert!(!ready_is_current(app.world()));
    assert!(app.world().resource::<EmbeddedSnapshotState>().pending);
    assert_eq!(waiting_reason(app.world()), Some("等待当前作者操作完整收口"));
    app.world_mut().resource_mut::<feathrise_editor_ui::ActiveNumericInputScrub>().entity = None;
    app.insert_resource(crate::authoring::TerrainBrushStroke { active: true, ..default() });
    app.update();
    assert!(!ready_is_current(app.world()), "现成同修订字节不能越过仍未收口的真实笔划");
    assert!(app.world().resource::<EmbeddedSnapshotState>().pending);
    app.world_mut().resource_mut::<crate::authoring::TerrainBrushStroke>().active = false;
    app.update();
    assert!(ready_is_current(app.world()));
    assert!(!app.world().resource::<EmbeddedSnapshotState>().pending);
    assert!(app.world().resource::<crate::resources::MapEditorNumericScrubMouseGuard>().blocked(),
        "数据事务完成不解除仍在显示的宿主浮层输入门");
    assert!(Arc::ptr_eq(app.world().resource::<EmbeddedSnapshotState>().ready.as_ref().unwrap(), &ready));
}

#[test]
fn empty_live_height_keeps_author_clock_in_complete_snapshot() {
    let mut app = app();
    let map = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        draft.manifest.terrain.kind = MapTerrainKind::SparseClipmap;
        draft.manifest.terrain.height_path = Some("maps/fixture/height_tiles.map_height_manifest.json".into());
        draft.terrain_dirty = false;
        draft.map_id.clone()
    };
    let mut live = SparseHeightLiveOverride::default();
    live.retarget(Some(map), 0);
    app.insert_resource(live);
    let source = capture::source_identity(app.world()).unwrap();
    let mut value = capture::Capture::new(app.world(), source).unwrap();
    assert!((0..32).any(|_| value.step(app.world()).unwrap()), "空高度捕获应在固定步数内完成");
    let encoded = encode::encode(value).unwrap();
    let snapshot: SceneSnapshot = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(snapshot.authoring_height_revision, Some(0), "空页与缺席的作者来源必须区分");
    assert!(snapshot.sparse_height_tiles.is_empty());
}

fn empty_vegetation_authoring(map: &MapId) -> MapVegetationAsset {
    serde_json::from_value(serde_json::json!({
        "schema_version": MAP_SCHEMA_VERSION, "map_id": map, "layers": [],
        "authoring": {"groups": [{
            "id": "base", "label": "基础植被", "mode": "additive", "output_merge": "maximum",
            "capacity": 1.0, "claim_threshold": 0.0, "selection_seed": 17,
            "selection_cell_meters": 8.0, "enabled": true
        }], "layers": []}
    }))
    .unwrap()
}

fn install_vegetation_compiler(app: &mut App) {
    use crate::authoring::{
        VegetationCompileState, poll_vegetation_compile, start_vegetation_compile,
    };
    app.init_resource::<crate::resources::MapEditorState>()
        .init_resource::<VegetationCompileState>()
        .add_systems(
            Update,
            (start_vegetation_compile, poll_vegetation_compile)
                .chain()
                .in_set(crate::MapEditorSystemSet::Tasks),
        );
}

#[test]
fn empty_container_snapshot_does_not_wait_for_a_nonexistent_vegetation_canvas() {
    use crate::authoring::{VegetationCompileSource, VegetationCompileState};
    let mut app = app();
    install_vegetation_compiler(&mut app);
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        // 对应正式新建空图：有空根植被文档，保留默认组，但没有任何独立地形画布。
        draft.manifest.terrain = MapTerrainDescriptor::empty();
        draft.manifest.terrains = Some(Vec::new().into());
        draft.manifest.world_min = [-0.5, -0.5];
        draft.manifest.world_max = [0.5, 0.5];
        draft.manifest.default_spawn = [0.0; 3];
        draft.vegetation = Some(empty_vegetation_authoring(&draft.map_id));
        draft.water = Some(MapWaterAsset {
            schema_version: MAP_SCHEMA_VERSION,
            map_id: draft.map_id.clone(),
            layers: Vec::new(),
            rivers: Vec::new(),
            waterfalls: Vec::new(),
        });
        session.vegetation_revision = 8;
        session.vegetation_compiled_revision = 7;
        session.vegetation_structure_revision = 11;
        session.vegetation_compiled_structure_revision = 10;
        assert!(
            VegetationCompileSource::capture(&session).is_none(),
            "正式编译器对无活动画布的容器不会产生回执，捕获门不能等待它"
        );
    }
    app.world_mut()
        .resource_mut::<EmbeddedSnapshotState>()
        .enabled = true;
    settle(&mut app);
    assert!(ready_is_current(app.world()));
    assert!(waiting_reason(app.world()).is_none());
    assert!(
        app.world()
            .resource::<VegetationCompileState>()
            .task
            .is_none()
    );
    let state = app.world().resource::<EmbeddedSnapshotState>();
    assert!(state.error.is_none(), "{:?}", state.error);
    let snapshot: SceneSnapshot =
        serde_json::from_slice(&state.ready.as_ref().unwrap().bytes).unwrap();
    assert!(snapshot.manifest.terrains.as_ref().unwrap().is_empty());
    assert!(snapshot.placements.placements.is_empty());
    assert!(snapshot.vegetation.is_none() && snapshot.entry.vegetation_path.is_none());
    assert!(
        snapshot.water.as_ref().unwrap().layers.is_empty(),
        "根水仍须独立捕获"
    );
    assert!(
        snapshot
            .asset_overrides
            .contains_key(snapshot.entry.water_path.as_ref().unwrap())
    );
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(
        (
            session.vegetation_revision,
            session.vegetation_compiled_revision
        ),
        (8, 7),
        "捕获不得伪造编译完成以绕过不存在的画布"
    );
    assert_eq!(
        (
            session.vegetation_structure_revision,
            session.vegetation_compiled_structure_revision
        ),
        (11, 10)
    );
    assert_eq!(session.successful_save_sequence, 0);
    assert!(session.undo.is_empty() && session.redo.is_empty());
}

#[test]
fn actual_vegetation_canvas_snapshot_waits_until_the_production_compiler_applies() {
    let mut app = app();
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        draft.vegetation = Some(empty_vegetation_authoring(&draft.map_id));
        session.vegetation_revision = 8;
        session.vegetation_compiled_revision = 7;
        session.vegetation_structure_revision = 11;
        session.vegetation_compiled_structure_revision = 10;
    }
    app.world_mut()
        .resource_mut::<EmbeddedSnapshotState>()
        .enabled = true;
    app.update();
    assert!(!ready_is_current(app.world()));
    assert_eq!(waiting_reason(app.world()), Some("等待植被作者层完整编译"));
    let state = app.world().resource::<EmbeddedSnapshotState>();
    assert!(
        state.pending && state.capture.is_none() && state.task.is_none() && state.ready.is_none()
    );
    install_vegetation_compiler(&mut app);
    settle(&mut app);
    assert!(ready_is_current(app.world()));
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(
        (
            session.vegetation_revision,
            session.vegetation_compiled_revision
        ),
        (8, 8)
    );
    assert_eq!(
        (
            session.vegetation_structure_revision,
            session.vegetation_compiled_structure_revision
        ),
        (11, 11)
    );
    assert_eq!(session.successful_save_sequence, 0);
    assert!(session.undo.is_empty() && session.redo.is_empty());
}

#[test]
fn saved_world_mode_does_no_snapshot_work() {
    let mut app = app();
    app.update();
    app.update();
    let state = app.world().resource::<EmbeddedSnapshotState>();
    assert!(
        !state.enabled
            && !state.pending
            && state.ready.is_none()
            && state.task.is_none()
            && state.capture.is_none()
    );
    assert_eq!(state.revision, 0);
}

#[test]
fn bounded_capture_restarts_on_revision_and_keeps_author_document_unchanged() {
    let mut app = app();
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .draft
        .as_mut()
        .unwrap()
        .placements
        .placements = (0..145).map(placement).collect();
    app.world_mut()
        .resource_mut::<EmbeddedSnapshotState>()
        .enabled = true;
    app.update();
    let state = app.world().resource::<EmbeddedSnapshotState>();
    assert!(state.ready.is_none());
    assert_eq!(
        state.capture.as_ref().unwrap().placements.placements.len(),
        64
    );
    {
        let mut source = app.world_mut().resource_mut::<EditorAuthoringSession>();
        source.revision += 1;
        source.draft.as_mut().unwrap().placements.placements[0].translation[0] = 7.;
        source.draft.as_mut().unwrap().terrain_dirty = true;
    }
    settle(&mut app);
    assert!(ready_is_current(app.world()));
    let state = app.world().resource::<EmbeddedSnapshotState>();
    let ready = state.ready.as_ref().unwrap();
    assert_eq!(ready.author_revision, 2);
    let snapshot: SceneSnapshot = serde_json::from_slice(&ready.bytes).unwrap();
    assert_eq!(snapshot.placements.placements.len(), 145);
    assert_eq!(snapshot.placements.placements[0].translation[0], 7.);
    let glb = &snapshot.asset_overrides["maps/EDGE-TEST/terrain/terrain.glb"];
    assert_eq!(&glb[..4], b"glTF");
    let source = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(source.revision, 4);
    assert_eq!(source.successful_save_sequence, 0);
    assert!(source.undo.is_empty() && source.redo.is_empty());
    assert!(
        source.draft.as_ref().unwrap().terrain_dirty,
        "捕获不能伪造保存成功或清除脏责任"
    );
}

#[test]
fn late_worker_cannot_replace_new_map_and_disabled_capture_does_not_publish_it() {
    let mut app = app();
    app.world_mut()
        .resource_mut::<EmbeddedSnapshotState>()
        .enabled = true;
    let identity = capture::source_identity(app.world()).unwrap();
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    {
        let mut state = app.world_mut().resource_mut::<EmbeddedSnapshotState>();
        state.source = Some(identity.clone());
        state.revision = 1;
        state.pending = true;
        state.task = Some(AsyncComputeTaskPool::get().spawn(async move {
            receiver.recv().unwrap();
            (
                identity,
                1,
                Ok(Arc::new(EmbeddedSnapshot {
                    map_id: "old".into(),
                    author_revision: 1,
                    bytes: Arc::new(b"old bytes".to_vec()),
                    pose_targets: Default::default(),
                })),
            )
        }));
    }
    app.world_mut()
        .resource_mut::<EmbeddedSnapshotState>()
        .enabled = false;
    app.update();
    assert!(
        app.world()
            .resource::<EmbeddedSnapshotState>()
            .task
            .is_some(),
        "取消采用不能提前释放仍在执行的唯一槽"
    );
    sender.send(()).unwrap();
    let until = Instant::now() + Duration::from_secs(5);
    while app
        .world()
        .resource::<EmbeddedSnapshotState>()
        .task
        .is_some()
    {
        app.update();
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(
        app.world()
            .resource::<EmbeddedSnapshotState>()
            .ready
            .is_none()
    );
    app.world_mut()
        .resource_mut::<EmbeddedSnapshotState>()
        .enabled = true;
    settle(&mut app);
    assert!(ready_is_current(app.world()));
    assert_eq!(
        app.world()
            .resource::<EmbeddedSnapshotState>()
            .ready
            .as_ref()
            .unwrap()
            .map_id,
        "EDGE-TEST"
    );
}

#[test]
fn missing_surface_source_is_explicit_failure_and_does_not_requeue_each_frame() {
    let mut app = app();
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .draft
        .as_mut()
        .unwrap()
        .surface = Some(MapSurfaceAsset {
        schema_version: MAP_SURFACE_SCHEMA_VERSION,
        map_id: MapId("EDGE-TEST".into()),
        resolution: [2, 2],
        layers: Vec::new(),
        weight_source_path: "maps/EDGE-TEST/weights.rgba8".into(),
        weight_texture_path: "maps/EDGE-TEST/weights.ktx2".into(),
        index_source_path: "maps/EDGE-TEST/indices.rgba32".into(),
        index_texture_path: "maps/EDGE-TEST/indices.ktx2".into(),
    });
    app.world_mut()
        .resource_mut::<EmbeddedSnapshotState>()
        .enabled = true;
    settle(&mut app);
    let error = app
        .world()
        .resource::<EmbeddedSnapshotState>()
        .error
        .clone();
    assert!(error.as_ref().is_some_and(|error| error.contains("权重")));
    for _ in 0..3 {
        app.update();
    }
    let state = app.world().resource::<EmbeddedSnapshotState>();
    assert_eq!(state.error, error);
    assert!(state.task.is_none() && state.ready.is_none());
    assert!(!ready_is_current(app.world()));
}

#[test]
fn root_and_parked_terrain_preserve_independent_masks_and_complete_surface_pairs() {
    use crate::authoring::terrain_documents::{TerrainCanvas, TerrainDocumentIdentity};
    let mut app = app();
    let (root_mask, private_mask, weights, indices, glb_path, surface_path) = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        let mut root_water =
            crate::authoring::water_asset_with_layer_for_tests(&draft.map_id, &draft.folder);
        root_water.layers[0].resolution = [2, 2];
        root_water.layers[0].water_level = 2.5;
        let root_mask = root_water.layers[0].mask_source_path.clone();
        draft
            .water_masks
            .insert(root_water.layers[0].id.clone(), Arc::new(vec![17; 4]));
        draft.water = Some(root_water);

        let mut object = draft.manifest.terrain_objects_for_edit()[0].clone();
        object.id = TerrainId::parse("private-test").unwrap();
        let folder = object.private_folder(&draft.map_id);
        object.terrain.glb_path = format!("{folder}/terrain.glb");
        object.surface_path = None;
        object.water_path = None;
        let mut private = TerrainCanvas::default();
        private.terrain_mesh = draft.terrain_mesh.clone();
        private.terrain_dirty = true;
        let mut water = crate::authoring::water_asset_with_layer_for_tests(&draft.map_id, &folder);
        water.layers[0].resolution = [2, 2];
        water.layers[0].water_level = 7.5;
        let private_mask = water.layers[0].mask_source_path.clone();
        private
            .water_masks
            .insert(water.layers[0].id.clone(), Arc::new(vec![231; 4]));
        private.water = Some(water);
        private.water_asset_path = format!("{folder}/{MAP_WATER_FILE_NAME}");
        let weights = format!("{folder}/weights.rgba8");
        let indices = format!("{folder}/indices.rgba32");
        private.surface = Some(MapSurfaceAsset {
            schema_version: MAP_SURFACE_SCHEMA_VERSION,
            map_id: draft.map_id.clone(),
            resolution: [2, 2],
            weight_source_path: weights.clone(),
            weight_texture_path: format!("{folder}/weights.ktx2"),
            index_source_path: indices.clone(),
            index_texture_path: format!("{folder}/indices.ktx2"),
            layers: vec![MapSurfaceLayer {
                id: "ground".into(),
                label: "地面".into(),
                preset_id: "ground".into(),
            }],
        });
        private.surface_weights = Some(Arc::new([255, 0, 0, 0].repeat(4)));
        private.surface_indices = Some(Arc::new(vec![0; 64]));
        let glb_path = object.terrain.glb_path.clone();
        draft.manifest.terrain = MapTerrainDescriptor::empty();
        draft.manifest.terrains = Some(vec![object.clone()].into());
        private.identity = Some(TerrainDocumentIdentity {
            definition: Arc::new(object.clone()),
            manifest: object.source_manifest(&draft.manifest),
            folder: folder.clone(),
        });
        draft
            .terrain_documents
            .values
            .insert(object.id, Arc::new(private));
        (
            root_mask,
            private_mask,
            weights,
            indices,
            glb_path,
            format!("{folder}/{MAP_SURFACE_FILE_NAME}"),
        )
    };
    app.world_mut()
        .resource_mut::<EmbeddedSnapshotState>()
        .enabled = true;
    settle(&mut app);
    let state = app.world().resource::<EmbeddedSnapshotState>();
    assert!(state.error.is_none(), "{:?}", state.error);
    let snapshot: SceneSnapshot =
        serde_json::from_slice(&state.ready.as_ref().unwrap().bytes).unwrap();
    assert_eq!(snapshot.water.as_ref().unwrap().layers[0].water_level, 2.5);
    assert_eq!(snapshot.asset_overrides[&root_mask].as_slice(), &[17; 4]);
    assert_eq!(
        snapshot.asset_overrides[&private_mask].as_slice(),
        &[231; 4]
    );
    assert_eq!(
        snapshot.asset_overrides[&weights].as_slice(),
        [255, 0, 0, 0].repeat(4)
    );
    assert_eq!(snapshot.asset_overrides[&indices].len(), 64);
    assert_eq!(&snapshot.asset_overrides[&glb_path][..4], b"glTF");
    let object = &snapshot.manifest.terrains.as_ref().unwrap()[0];
    assert_eq!(object.surface_path.as_deref(), Some(surface_path.as_str()));
    let private_water: MapWaterAsset =
        serde_json::from_slice(&snapshot.asset_overrides[object.water_path.as_ref().unwrap()])
            .unwrap();
    assert_eq!(private_water.layers[0].water_level, 7.5);
    for (path, bytes) in &snapshot.asset_overrides {
        if path.ends_with(".ktx2") {
            assert_eq!(&bytes[..12], b"\xabKTX 20\xbb\r\n\x1a\n");
        }
    }
    assert_eq!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .successful_save_sequence,
        0
    );
}
