//! 首版拼接沿用正式异步采用、预览取消、场景级历史与磁盘协议。
use super::*;
use std::{
    collections::BTreeSet,
    sync::Arc,
    time::{Duration, Instant},
};

fn fixture() -> (App, Vec<SceneTerrainObject>) {
    let mut app = app();
    select_first(&mut app);
    let mut a = native_object(&app).clone();
    let map = app
        .world()
        .resource::<EditorAuthoringSession>()
        .draft
        .as_ref()
        .unwrap()
        .scene_id
        .clone();
    a.world_min = [0.; 2];
    a.world_max = [64.; 2];
    a.terrain = SceneTerrainDescriptor::empty();
    a.terrain.kind = SceneTerrainKind::SparseClipmap;
    a.terrain.height_scale = 32.;
    a.terrain.resolution = [129; 2];
    a.terrain.collision_lods = default_collision_lods_for_resolution(a.terrain.resolution);
    a.terrain.height_path = Some(format!(
        "scenes/{map}/terrains/{}/height_tiles.scene_height_manifest.json",
        a.id.as_str()
    ));
    let mut b = a.clone();
    b.id = TerrainId::parse("terrain-b").unwrap();
    b.transform.translation[0] = 64.;
    b.terrain.resolution = [33; 2];
    b.terrain.collision_lods = default_collision_lods_for_resolution(b.terrain.resolution);
    b.terrain.height_path = Some(format!(
        "scenes/{map}/terrains/{}/height_tiles.scene_height_manifest.json",
        b.id.as_str()
    ));
    let objects = vec![a, b];
    let mut original = objects.clone();
    original[0].transform.translation[0] = 0.75;
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .draft
        .as_mut()
        .unwrap()
        .manifest
        .terrains = Some(original.into());
    let mut result = objects;
    let (a, b) = clipmap_stitch_pair(
        &result[0],
        ClipmapEdge::East,
        &result[1],
        ClipmapEdge::West,
        "test-stitch".into(),
        |d| Ok(1. + d * 0.03),
    )
    .unwrap();
    result[0].clipmap_stitches = Arc::from([a]);
    result[1].clipmap_stitches = Arc::from([b]);
    (app, result)
}
fn queue_preview(app: &mut App, result: Vec<SceneTerrainObject>) {
    let session = app.world().resource::<EditorAuthoringSession>();
    let draft = session.draft.as_ref().unwrap();
    let map = draft.scene_id.clone();
    let before = SceneTerrainCollection::capture(&draft.manifest);
    let revision = session.revision;
    let selected = result[0].id.clone();
    app.world_mut()
        .resource_mut::<TerrainObjectsEditorState>()
        .pending = Some(types::Pending {
        preview_stitch: true,
        stitching: true,
        stroke: None,
        target: Some(selected.clone()),
        affects_terrain: true,
        lod_navigation: None,
        scene_id: map,
        revision,
        before,
        task: bevy::tasks::IoTaskPool::get().spawn(async move {
            Ok(types::Prepared {
                stitch_bytes: 4096,
                changed: true,
                objects: result.into(),
                selected: Some(selected),
                selected_index: Some(0),
            })
        }),
    });
    let start = Instant::now();
    while app
        .world()
        .resource::<TerrainObjectsEditorState>()
        .stitch_preview
        .is_none()
    {
        app.update();
        assert!(start.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(1));
    }
}
#[test]
fn stitch_preview_confirm_has_one_bounded_command_and_saves_both_sides() {
    let (mut app, result) = fixture();
    let before = app
        .world()
        .resource::<EditorAuthoringSession>()
        .draft
        .as_ref()
        .unwrap()
        .manifest
        .clone();
    queue_preview(&mut app, result);
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    );
    assert_eq!(
        serde_json::to_value(
            &app.world()
                .resource::<EditorAuthoringSession>()
                .draft
                .as_ref()
                .unwrap()
                .manifest
        )
        .unwrap(),
        serde_json::to_value(&before).unwrap()
    );
    assert!(
        app.world()
            .resource::<feathrise_scene::TerrainObjectTransformPreview>()
            .pose
            .is_some()
    );
    app.world_mut()
        .resource_mut::<TerrainObjectsEditorState>()
        .stitch_apply = true;
    complete(&mut app);
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.undo.len(), 1);
    let EditorCommand::ClipmapStitchesChanged {
        before,
        after,
        heap_bytes,
    } = &session.undo[0]
    else {
        panic!("拼接必须采用整组有界命令");
    };
    assert!(*heap_bytes > 0);
    let mut restored = session.draft.as_ref().unwrap().manifest.clone();
    before.apply(&mut restored);
    assert_eq!(
        restored.terrains.as_ref().unwrap()[0].transform.translation[0],
        0.75
    );
    assert!(
        restored
            .terrains
            .as_ref()
            .unwrap()
            .iter()
            .all(|o| o.clipmap_stitches.is_empty())
    );
    after.apply(&mut restored);
    assert_eq!(
        restored.terrains.as_ref().unwrap()[0].transform.translation[0],
        0.0
    );
    assert!(
        restored
            .terrains
            .as_ref()
            .unwrap()
            .iter()
            .all(|o| o.clipmap_stitches.len() == 1)
    );
    let bytes = serde_json::to_vec(&restored).unwrap();
    let loaded: SceneManifestAsset = serde_json::from_slice(&bytes).unwrap();
    validate_terrain_objects(&loaded).unwrap();
    let objects = loaded.terrains.as_ref().unwrap();
    assert_eq!(resolve_clipmap_stitches(&objects[0], objects).len(), 1);
    assert_eq!(objects[0].terrain.resolution, [129; 2]);
    assert_eq!(objects[1].terrain.resolution, [33; 2]);
}
#[test]
fn stitch_cancel_save_undo_and_late_peer_lock_do_not_modify_author_data() {
    for trigger in 0..5 {
        let (mut app, result) = fixture();
        let before = app
            .world()
            .resource::<EditorAuthoringSession>()
            .draft
            .as_ref()
            .unwrap()
            .manifest
            .clone();
        let peer = result[1].id.clone();
        queue_preview(&mut app, result);
        match trigger {
            0 => {
                app.world_mut()
                    .resource_mut::<TerrainObjectsEditorState>()
                    .stitch_cancel = true
            }
            1 => {
                app.world_mut()
                    .resource_mut::<SceneEditorState>()
                    .save_requested = true
            }
            2 => {
                app.world_mut()
                    .resource_mut::<SceneEditorState>()
                    .undo_requested = true
            }
            3 => {
                let mut state = app.world_mut().resource_mut::<TerrainObjectsEditorState>();
                state.locked.insert(peer);
                state.stitch_apply = true;
            }
            _ => {
                // 模拟来源换代后才完成的后台结果；不经过持有预览的正常取消入口。
                let mut state = app.world_mut().resource_mut::<TerrainObjectsEditorState>();
                let held = state.stitch_preview.take().unwrap();
                let mut pending = held.pending;
                let prepared = held.prepared;
                pending.task = bevy::tasks::IoTaskPool::get().spawn(async move { Ok(prepared) });
                state.pending = Some(pending);
                drop(state);
                app.world_mut()
                    .resource_mut::<EditorAuthoringSession>()
                    .revision += 1;
            }
        }
        complete(&mut app);
        let session = app.world().resource::<EditorAuthoringSession>();
        assert!(session.undo.is_empty());
        assert_eq!(
            serde_json::to_value(&session.draft.as_ref().unwrap().manifest).unwrap(),
            serde_json::to_value(&before).unwrap()
        );
        assert!(
            app.world()
                .resource::<feathrise_scene::TerrainObjectTransformPreview>()
                .pose
                .is_none()
        );
    }
}
#[test]
fn stitch_unlink_updates_both_sides_in_one_command() {
    let (mut app, result) = fixture();
    let id = result[0].id.clone();
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .draft
        .as_mut()
        .unwrap()
        .manifest
        .terrains = Some(result.into());
    begin(&mut app, Mutation::Unstitch(id));
    complete(&mut app);
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.undo.len(), 1);
    assert!(
        session
            .draft
            .as_ref()
            .unwrap()
            .manifest
            .terrains
            .as_ref()
            .unwrap()
            .iter()
            .all(|o| o.clipmap_stitches.is_empty())
    );
    assert!(matches!(
        &session.undo[0],
        EditorCommand::ClipmapStitchesChanged { .. }
    ));
}
#[test]
fn stitch_preparation_reads_only_reference_author_source_in_isolated_directory() {
    let (app, _) = fixture();
    let session = app.world().resource::<EditorAuthoringSession>();
    let draft = session.draft.as_ref().unwrap();
    let mut objects = draft.manifest.terrains.as_ref().unwrap().to_vec();
    objects[0].transform.translation[0] = 0.75;
    let temp = std::env::temp_dir().canonicalize().unwrap();
    let root = temp.join(format!(
        "bevy-clipmap-stitch-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&root).unwrap();
    let spec = SparseHeightSpec::new([0.; 2], [64.; 2], 2., 0., 32.).unwrap();
    let mut document = SparseHeightDocument::create_for_terrain(
        &root,
        draft.scene_id.clone(),
        Some(objects[1].id.clone()),
        spec,
    )
    .unwrap();
    document.save_dirty().unwrap();
    let path = root.join(objects[1].terrain.height_path.as_ref().unwrap());
    let before = std::fs::read(&path).unwrap();
    let result = bevy::tasks::block_on(super::super::stitches::prepare_at(
        root.clone(),
        draft.scene_id.clone(),
        objects.clone().into(),
        objects[0].id.clone(),
        BTreeSet::new(),
        1,
    ))
    .unwrap();
    assert!(result[0].transform.translation[0].abs() < 0.0001);
    assert_eq!(result[0].clipmap_stitches.len(), 1);
    assert_eq!(result[1].clipmap_stitches.len(), 1);
    assert_eq!(std::fs::read(path).unwrap(), before);
    assert!(
        !root
            .join(objects[0].terrain.height_path.as_ref().unwrap())
            .exists()
    );
    assert_eq!(root.canonicalize().unwrap().parent(), Some(temp.as_path()));
    std::fs::remove_dir_all(root).unwrap(); // 路径已核对为本测试创建的独占临时目录。
}
