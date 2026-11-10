//! 原子采用系统回归：建筑起点失败时高度、修订和保存责任均不修改。
use super::*;
use bevy::ecs::system::RunSystemOnce;

#[test]
fn stale_building_is_rejected_before_installing_any_page() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()));
    let mut draft =
        crate::authoring::terrain::tests::test_heightfield_draft(std::sync::Arc::new(vec![0; 121]));
    draft.manifest.terrain.kind = feathrise_scene_content::SceneTerrainKind::SparseClipmap;
    let scene_id = draft.scene_id.clone();
    app.insert_resource(SceneEditorState {
        open: true,
        selected_scene_id: Some(scene_id),
        ..default()
    });
    let mut session = EditorAuthoringSession::default();
    session.draft = Some(draft);
    app.insert_resource(session)
        .init_resource::<SparseTerrainAuthoringState>()
        .init_resource::<SparseHeightLiveOverride>()
        .init_resource::<PlacementInstanceIndex>();
    let root = std::env::temp_dir().join(format!(
        "sparse-building-stale-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let id = feathrise_scene_content::SceneId::parse("MAP-stale-platform").unwrap();
    let spec = feathrise_scene_content::SparseHeightSpec::new([0., 0.], [64., 64.], 1., -20., 80.).unwrap();
    let mut document =
        feathrise_scene_content::SparseHeightDocument::create_empty(&root, id.clone(), spec).unwrap();
    document.save_dirty().unwrap();
    let mut author = feathrise_scene_content::SparseTerrainAuthorSession::load(&root, &id).unwrap();
    let edit = author
        .prepare_changes_loaded(std::sync::Arc::new(Vec::new()), true)
        .unwrap();
    let record = ScenePlacementRecord {
        mesh_lod: None,
        terrain_binding: None,
        editor: Default::default(),
        terrain_patch: None,
        id: "stale".into(),
        asset_path: "gltf/house.glb".into(),
        asset_target: Default::default(),
        building_preset_id: Some("house".into()),
        courtyard: None,
        translation: [5., 0., 5.],
        height_mode: ScenePlacementHeightMode::FixedHeight,
        terrain_offset: 0.,
        yaw_degrees: 0.,
        scale: [1.; 3],
        water_surfaces: Vec::new(),
    };
    let change = BuildingPlacementChange::Added {
        record_index: 9,
        record,
    };
    let mut input = Some((change, edit));
    let before = app.world().resource::<EditorAuthoringSession>().revision;
    let result = app.world_mut().run_system_once(
        move |mut commands: Commands,
              assets: Res<AssetServer>,
              index: Res<PlacementInstanceIndex>,
              roots: Query<(Entity, &ScenePlacementInstance)>,
              mut editor: ResMut<SceneEditorState>,
              mut session: ResMut<EditorAuthoringSession>,
              mut sparse: ResMut<SparseTerrainAuthoringState>,
              mut live: ResMut<SparseHeightLiveOverride>| {
            let (change, edit) = input.take().unwrap();
            apply_transaction(
                &mut commands,
                &assets,
                &index,
                &roots,
                &mut editor,
                &mut session,
                &mut sparse,
                &mut live,
                &change,
                edit,
                false,
            )
        },
    );
    assert!(result.unwrap().unwrap_err().contains("尾槽"));
    assert_eq!(
        app.world().resource::<EditorAuthoringSession>().revision,
        before
    );
    assert!(!app.world().resource::<SceneEditorState>().dirty);
    assert_eq!(
        app.world()
            .resource::<SparseHeightLiveOverride>()
            .tile_count(),
        0
    );
    std::fs::remove_dir_all(root).unwrap();
}
