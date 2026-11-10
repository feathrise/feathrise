//! 正式保存让出作者覆盖后恢复同修订或更新草稿；预检恢复、迟到结果与失败缓存保持独立。
use super::*;
use std::time::{Duration, Instant};

fn fixture() -> (App, SceneId) {
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    draft.manifest.terrain.kind = SceneTerrainKind::SparseClipmap;
    let map = draft.scene_id.clone();
    draft.water = Some(SceneWaterAsset { scene_id: map.clone(), ..default() });
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
        .init_asset::<feathrise_scene_content::SceneManifestAsset>().init_asset::<SceneWaterAsset>()
        .init_resource::<Assets<Image>>().init_resource::<LiveClipmapWaterState>()
        .init_resource::<DynamicClipmapWaterDraft>().init_resource::<WaterFileHandoff>()
        .insert_resource(feathrise_scene_content::ActiveDynamicScene { scene_id: Some(map.clone()) })
        .insert_resource(EditorAuthoringSession { draft: Some(draft), water_revision: 1, ..default() })
        .add_systems(Update, sync_live_clipmap_water);
    until(&mut app, |app| published(app, &map) == Some(1));
    (app, map)
}
fn until(app: &mut App, done: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done(app) {
        assert!(Instant::now() < deadline, "正式门结束后水草稿应恢复发布");
        app.update(); std::thread::sleep(Duration::from_millis(1));
    }
}
fn published(app: &App, map: &SceneId) -> Option<u64> {
    app.world().resource::<DynamicClipmapWaterDraft>().for_scene(map).map(|snapshot| snapshot.source_revision)
}
fn edit(app: &mut App, revision: u64) {
    let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
    session.water_revision = revision;
    let draft = session.draft.as_mut().unwrap();
    let mut water = crate::authoring::water_asset_with_layer_for_tests(&draft.scene_id, &draft.folder);
    water.layers[0].resolution = [2; 2];
    let mut second = water.layers[0].clone(); second.id = "second-lake".into();
    water.layers.push(second);
    for layer in &water.layers {
        draft.water_masks.insert(layer.id.clone(), Arc::new(vec![revision as u8; 4]));
    }
    draft.water = Some(water);
}
fn begin_save(app: &mut App, map: &SceneId, revision: u64) {
    let mut handoff = WaterFileHandoff::default();
    handoff.begin(app.world().resource::<AssetServer>(), map.clone(), revision, 0, 0,
        "scenes/MAP-live-water-save/scene.scene_manifest.json", "scenes/MAP-live-water-save/water.scene_water.json",
        &[], &[], &[], None, false, Duration::ZERO).unwrap();
    assert_eq!(handoff.formal_water_revision_for_canvas(map, None), Some(revision));
    app.insert_resource(handoff);
}

#[test]
fn newer_applied_or_staged_water_republishes_after_an_older_formal_save_finishes() {
    for staged in [false, true] {
        let (mut app, map) = fixture();
        edit(&mut app, 2);
        until(&mut app, |app| if staged {
            app.world().resource::<LiveClipmapWaterState>().terrain.staging.is_some()
        } else { published(app, &map) == Some(2) });
        begin_save(&mut app, &map, 1);
        app.update();
        assert_eq!(published(&app, &map), None, "正式文件采用期间让出作者覆盖");
        let state = app.world().resource::<LiveClipmapWaterState>();
        assert!(state.terrain.task.is_none() && state.terrain.staging.is_none());
        assert!(state.terrain.applied.is_none() && state.terrain.attempted.is_none(), "被撤回的新候选不能留下永久跳过缓存");
        app.insert_resource(WaterFileHandoff::default()); // 注入既有交接资源的完成边界；不替代正式文件门自身的回归。
        until(&mut app, |app| published(app, &map) == Some(2));
        assert_eq!(app.world().resource::<LiveClipmapWaterState>().terrain.applied, Some((map, 2)));
    }
}

#[test]
fn formal_save_keeps_the_inflight_task_identity_and_newer_author_wins_after_release() {
    let (mut app, map) = fixture();
    edit(&mut app, 2);
    app.update();
    let original = app.world_mut().resource_mut::<LiveClipmapWaterState>().terrain.task.take().unwrap();
    let (release, receiver) = std::sync::mpsc::channel();
    app.world_mut().resource_mut::<LiveClipmapWaterState>().terrain.task = Some(AsyncComputeTaskPool::get().spawn(async move {
        let _ = receiver.recv(); // 只延后真实worker完成值，原定义/像素/修订保持原样。
        original.await
    }));
    begin_save(&mut app, &map, 1);
    app.update();
    edit(&mut app, 3);
    app.update();
    {
        let state = app.world().resource::<LiveClipmapWaterState>();
        assert!(state.terrain.task.is_some());
        assert_eq!(state.terrain.attempted, Some((map.clone(), 2)), "保存门不能把仍在途的2号任务改挂到3号作者");
    }
    app.insert_resource(WaterFileHandoff::default());
    app.update();
    assert_eq!(app.world().resource::<LiveClipmapWaterState>().terrain.attempted, Some((map.clone(), 2)));
    release.send(()).unwrap();
    until(&mut app, |app| {
        assert_ne!(published(app, &map), Some(2), "迟到旧任务不能抢占保存期间的新一笔");
        published(app, &map) == Some(3)
    });
}

/// 这里只替代正式GPU采用回执；草稿R8准备、完整发布和保存预检信用均运行正式系统。
fn accept_published_water_for_preview(
    active: Res<feathrise_scene_content::ActiveDynamicScene>,
    draft: Res<DynamicClipmapWaterDraft>,
    mut status: ResMut<feathrise_scene::DynamicWaterStatus>,
) {
    let Some(map) = active.scene_id.as_ref() else { return; };
    let revision = draft.revision_for_scene(map);
    if status.scene_id.as_ref() != Some(map) || status.input_draft_revision != revision {
        status.desired_generation = status.desired_generation.wrapping_add(1).max(1);
    }
    status.scene_id = Some(map.clone());
    status.input_draft_revision = revision;
    status.applied_generation = status.desired_generation;
    status.phase = feathrise_scene::DynamicWaterPhase::Ready;
}

#[test]
fn same_author_water_republishes_and_restores_preflight_after_each_formal_save() {
    use crate::authoring::{WaterBrushStroke, WaterPreviewState};
    use crate::resources::SceneEditorState;
    let (mut app, map) = fixture();
    app.insert_resource(SceneEditorState { open: true, selected_scene_id: Some(map.clone()), ..default() })
        .init_resource::<WaterBrushStroke>().init_resource::<WaterPreviewState>()
        .init_resource::<feathrise_scene::DynamicWaterStatus>()
        .add_systems(Update, (accept_published_water_for_preview,
            crate::authoring::start_water_preview_rebuild).chain().after(sync_live_clipmap_water));
    edit(&mut app, 2);
    until(&mut app, |app| app.world().resource::<WaterPreviewState>().ready_for(&map, 2, 0));
    for _ in 0..2 {
        let before = app.world().resource::<DynamicClipmapWaterDraft>().revision_for_scene(&map).unwrap();
        begin_save(&mut app, &map, 2);
        for _ in 0..4 {
            app.update();
            assert_eq!(published(&app, &map), None, "正式门有效期间绝不能发布同内容作者覆盖");
            assert!(!app.world().resource::<WaterPreviewState>().ready_for(&map, 2, 0),
                "正式文件Ready不等于仍存在同修订作者发布");
        }
        app.insert_resource(WaterFileHandoff::default()); // 模拟正式文件门已完整结束，不伪造作者发布或预检信用。
        until(&mut app, |app| app.world().resource::<WaterPreviewState>().ready_for(&map, 2, 0));
        let published = app.world().resource::<DynamicClipmapWaterDraft>();
        let snapshot = published.for_scene(&map).unwrap();
        assert_eq!(snapshot.source_revision, 2);
        assert_eq!(snapshot.definition.layers.len(), 2);
        assert_eq!(snapshot.sources.len(), 2);
        assert_eq!(snapshot.masks.len(), 2);
        for source in snapshot.sources.values() { assert_eq!(source.as_ref(), &[2; 4]); }
        assert_ne!(published.revision_for_scene(&map), Some(before), "新发布必须拥有独立采用代次");
        assert_eq!(app.world().resource::<EditorAuthoringSession>().water_revision, 2,
            "恢复下一次保存预检不能靠伪造作者修改推进修订");
    }
}

#[test]
fn same_revision_formal_gate_keeps_one_task_and_rejects_a_late_superseded_result() {
    for superseded in [false, true] {
        let (mut app, map) = fixture();
        edit(&mut app, 2);
        app.update();
        let original = app.world_mut().resource_mut::<LiveClipmapWaterState>().terrain.task.take().unwrap();
        // 真正worker继续运行，只暂缓唯一槽的观察；pending future不会阻塞任何计算线程。
        app.world_mut().resource_mut::<LiveClipmapWaterState>().terrain.task = Some(
            AsyncComputeTaskPool::get().spawn(std::future::pending::<Result<Prepared, String>>()));
        begin_save(&mut app, &map, 2);
        app.update();
        if superseded { edit(&mut app, 3); }
        for _ in 0..4 {
            app.update();
            assert_eq!(published(&app, &map), None);
            let state = app.world().resource::<LiveClipmapWaterState>();
            assert!(state.terrain.task.is_some());
            assert_eq!(state.terrain.attempted, Some((map.clone(), 2)), "正式门不能改挂在途任务身份");
        }
        app.insert_resource(WaterFileHandoff::default());
        app.update();
        assert_eq!(published(&app, &map), None, "原单飞槽未结清前不能提前发布或另开任务");
        let paused = app.world_mut().resource_mut::<LiveClipmapWaterState>().terrain.task.replace(original);
        drop(paused); // 只撤销没有数据的测试占槽，真实完成值仍由正式轮询校验。
        let expected = if superseded { 3 } else { 2 };
        until(&mut app, |app| {
            if superseded { assert_ne!(published(app, &map), Some(2), "迟到旧定义与R8不能抢回发布"); }
            published(app, &map) == Some(expected)
        });
    }
}

#[test]
fn formal_gate_does_not_retry_or_repeat_an_unchanged_author_preparation_failure() {
    use crate::resources::SceneEditorState;
    let (mut app, map) = fixture();
    app.init_resource::<SceneEditorState>();
    edit(&mut app, 2);
    app.world_mut().resource_mut::<EditorAuthoringSession>().draft.as_mut().unwrap().water_masks.clear();
    until(&mut app, |app| app.world().resource::<LiveClipmapWaterState>().failure_for(&map, 2).is_some());
    let failure = app.world().resource::<LiveClipmapWaterState>().failure_for(&map, 2).unwrap().to_owned();
    begin_save(&mut app, &map, 1); // 旧修订IO完成时，新作者修订可能已在准备阶段明确失败。
    app.update();
    app.insert_resource(WaterFileHandoff::default());
    app.world_mut().resource_mut::<SceneEditorState>().status = "其他操作的反馈".into();
    for _ in 0..16 {
        app.update();
        let state = app.world().resource::<LiveClipmapWaterState>();
        assert!(state.terrain.task.is_none() && state.terrain.staging.is_none(), "同一失败身份不能在正式门结束后重新排队");
        assert_eq!(state.failure_for(&map, 2), Some(failure.as_str()));
        assert_eq!(app.world().resource::<SceneEditorState>().status, "其他操作的反馈",
            "缓存失败不能反复覆盖状态或再次产生日志");
    }
    edit(&mut app, 3);
    until(&mut app, |app| published(app, &map) == Some(3));
    assert!(app.world().resource::<LiveClipmapWaterState>().failure_for(&map, 3).is_none());
}

#[test]
fn independent_root_stays_published_while_private_canvas_changes_and_uses_one_worker() {
    use crate::authoring::terrain_documents::TerrainDocumentIdentity;
    let (mut app, map) = fixture();
    edit(&mut app, 2);
    let object = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        let object = draft.manifest.terrain_objects_for_edit()[0].clone();
        draft.manifest.terrain = feathrise_scene_content::SceneTerrainDescriptor::empty();
        draft.manifest.terrains = Some(Arc::from([]));
        object
    };
    until(&mut app, |app| app.world().resource::<DynamicClipmapWaterDraft>()
        .root_for_scene(&map).is_some_and(|root| root.source_revision == 2));
    let published_root_revision = app.world().resource::<DynamicClipmapWaterDraft>().root_revision_for_scene(&map);
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        draft.terrain_documents.root_water = Some(crate::authoring::terrain_documents::RootWaterCanvas {
            revision: 2, water: draft.water.take(), water_masks: std::mem::take(&mut draft.water_masks),
            ..default()
        });
        draft.terrain_documents.active = Some(TerrainDocumentIdentity {
            manifest: object.source_manifest(&draft.manifest), folder: object.private_folder(&map),
            definition: Arc::new(object),
        });
    }
    edit(&mut app, 9); // 兄弟私有层有同名稳定层ID，但绝不能替换根水R8或根修订。
    until(&mut app, |app| {
        let state = app.world().resource::<LiveClipmapWaterState>();
        assert!(!(state.root.task.is_some() && state.terrain.task.is_some()));
        let output = app.world().resource::<DynamicClipmapWaterDraft>();
        assert_eq!(output.root_revision_for_scene(&map), published_root_revision);
        assert_eq!(output.root_for_scene(&map).unwrap().sources["lake"].as_ref(), &[2; 4]);
        published(app, &map) == Some(9)
    });
    begin_save(&mut app, &map, 2);
    app.update();
    let output = app.world().resource::<DynamicClipmapWaterDraft>();
    assert!(output.root_for_scene(&map).is_none(), "根正式门只撤回根作者覆盖");
    assert_eq!(output.for_scene(&map).unwrap().source_revision, 9);
    app.insert_resource(WaterFileHandoff::default());
    until(&mut app, |app| app.world().resource::<DynamicClipmapWaterDraft>()
        .root_for_scene(&map).is_some_and(|root| root.source_revision == 2));
}
