//! 独占临时高度场景与最小ECS联合事务夹具。
use super::*;

pub(super) fn fixture() -> (
    App,
    std::path::PathBuf,
    ScenePlacementRecord,
    Arc<Vec<SparseTerrainHeightChange>>,
) {
    let (root, scene_id) = crate::authoring::sparse_terrain::tests::test_scene();
    let author = SparseTerrainAuthorSession::load(&root, &scene_id).unwrap();
    let changes = Arc::new(
        [(255, 255), (256, 255), (255, 256), (256, 256)]
            .into_iter()
            .map(|(x, z)| SparseTerrainHeightChange {
                x,
                z,
                before: author.spec().flat_height_code(),
                after: author.spec().encode_height(5.).unwrap(),
            })
            .collect(),
    );
    let mut draft =
        crate::authoring::terrain::tests::test_heightfield_draft(Arc::new(vec![0; 121]));
    draft.scene_id = scene_id.clone();
    draft.manifest.id = scene_id.clone();
    draft.placements.scene_id = scene_id.clone();
    draft.manifest.world_min = [-32.; 2];
    draft.manifest.world_max = [32.; 2];
    draft.manifest.terrain.kind = SceneTerrainKind::SparseClipmap;
    draft.manifest.terrain.glb_path.clear();
    draft.terrain_mesh = None;
    draft.terrain_editing_index = None;
    draft.heights = None;
    let record = ScenePlacementRecord {
        mesh_lod: None,
        terrain_binding: None,
        editor: Default::default(),
        terrain_patch: None,
        id: "platform-house".into(),
        asset_path: "gltf/house.glb".into(),
        asset_target: Default::default(),
        building_preset_id: Some("house".into()),
        courtyard: None,
        translation: [0., 5., 0.],
        height_mode: ScenePlacementHeightMode::FixedHeight,
        terrain_offset: 0.,
        yaw_degrees: 0.,
        scale: [1.; 3],
        water_surfaces: Vec::new(),
    };
    let mut live = SparseHeightLiveOverride::default();
    live.retarget(Some(scene_id.clone()), 0);
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
        .init_asset::<bevy::world_serialization::WorldAsset>()
        .insert_resource(SceneEditorState {
            open: true,
            selected_scene_id: Some(scene_id.clone()),
            ..default()
        })
        .insert_resource(EditorAuthoringSession {
            draft: Some(draft),
            ..default()
        })
        .insert_resource(SparseTerrainAuthoringState {
            desired_scene_id: Some(scene_id),
            author: Some(author),
            ..default()
        })
        .insert_resource(live)
        .init_resource::<PlacementInstanceIndex>()
        .init_resource::<GlobalEnvironmentAuthoringState>()
        .init_resource::<crate::authoring::AuthoringRetirement>();
    (app, root, record, changes)
}

pub(super) fn prepare(
    app: &mut App,
    changes: Arc<Vec<SparseTerrainHeightChange>>,
    redo: bool,
) -> SparseTerrainPreparedEdit {
    app.world_mut()
        .resource_mut::<SparseTerrainAuthoringState>()
        .author
        .as_mut()
        .unwrap()
        .prepare_changes_loaded(changes, redo)
        .unwrap()
}

pub(super) fn apply(
    app: &mut App,
    placement: BuildingPlacementChange,
    edit: SparseTerrainPreparedEdit,
) -> Result<Arc<Vec<SparseTerrainHeightChange>>, String> {
    let mut input = Some((placement, edit));
    app.world_mut()
        .run_system_once(
            move |mut commands: Commands,
                  assets: Res<AssetServer>,
                  index: Res<PlacementInstanceIndex>,
                  roots: Query<(Entity, &ScenePlacementInstance)>,
                  mut editor: ResMut<SceneEditorState>,
                  mut session: ResMut<EditorAuthoringSession>,
                  mut sparse: ResMut<SparseTerrainAuthoringState>,
                  mut live: ResMut<SparseHeightLiveOverride>| {
                let (placement, edit) = input.take().unwrap();
                crate::authoring::buildings::apply_sparse_building_transaction(
                    &mut commands,
                    &assets,
                    &index,
                    &roots,
                    &mut editor,
                    &mut session,
                    &mut sparse,
                    &mut live,
                    &placement,
                    edit,
                    false,
                )
            },
        )
        .unwrap()
}

pub(super) fn request(app: &mut App, undoing: bool) -> SparseBuildingHistoryRequest {
    let session = app.world().resource::<EditorAuthoringSession>();
    let top = if undoing {
        session.undo.last()
    } else {
        session.redo.last()
    };
    let Some(command) = top else {
        panic!("必须共同持有建筑与高度栈顶")
    };
    let EditorCommand::BuildingSparseTerrainChanged(change) = command.unscoped() else {
        panic!("必须共同持有建筑与高度栈顶")
    };
    SparseBuildingHistoryRequest {
        undoing,
        target: history::capture_target(
            command,
            app.world().resource::<SparseTerrainAuthoringState>(),
        ),
        change: change.clone(),
        author_revision: session.revision,
        terrain_revision: session.terrain_revision,
        structure_revision: session.placement_structure_revision,
        ground_content: None,
    } // 最小World没有渲染器，生产入口另测缺来源等待。
}

pub(super) fn replay_in_background(app: &mut App, undoing: bool) {
    let request = request(app, undoing);
    start(
        &mut app
            .world_mut()
            .resource_mut::<SparseTerrainAuthoringState>(),
        request,
    );
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .saving = true;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while app
        .world()
        .resource::<SparseTerrainAuthoringState>()
        .task
        .is_some()
    {
        assert!(std::time::Instant::now() < deadline, "有界局部任务应结束");
        app.world_mut()
            .run_system_once(poll_sparse_terrain_task)
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    {
        let sparse = app.world().resource::<SparseTerrainAuthoringState>();
        assert!(sparse.author.is_some(), "联合历史必须先归还原作者源，再交给正式高度采用门");
        assert!(!sparse.external_lease, "历史后台槽不借用建筑放置的外部租约");
        assert!(sparse.building_history_ready.is_some());
    }
    app.world_mut()
        .run_system_once(poll_sparse_building_history)
        .unwrap();
    assert!(!app.world().resource::<EditorAuthoringSession>().saving);
}
