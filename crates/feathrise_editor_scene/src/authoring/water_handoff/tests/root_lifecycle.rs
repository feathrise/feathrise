//! 真实交接轮询验证根正式输入先完成、再交接私有水；作者Ready不能提前解除关闭责任。
use super::*;
use feathrise_scene_content::TerrainId;
use crate::authoring::resources::TerrainWaterSaveHandoffSnapshot;

#[test]
fn root_formal_ready_then_private_files_complete_the_same_polling_transaction() {
    let map = SceneId::parse("MAP-water-handoff").unwrap();
    let id = TerrainId::parse("private").unwrap();
    let mut handoff = WaterFileHandoff { pending: Some(pending_handoff(&map)), ..default() };
    handoff.pending.as_mut().unwrap().gate.all_inputs_succeeded_frame = Some(u32::MAX);
    let mut targets = vec![TerrainWaterSaveHandoffSnapshot {
        terrain_id: id.clone(), water_asset_path: "scenes/MAP-water-handoff/private/water.scene_water.json".into(),
        baselines: Vec::new(), dirty_layers: Vec::new(), catalog_registration_required: false,
        retry_required: false, runtime_required: false, // 已禁用私有目标只等自己的正式文件证据。
    }];
    handoff.attach_terrain_batch(&map, 19, "scenes/MAP-water-handoff/scene.scene_manifest.json",
        &mut targets, &mut [id.clone()].into(), None).unwrap();
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .add_message::<feathrise_scene::TerrainWaterRebuildRequest>()
        .add_message::<AssetEvent<SceneManifestAsset>>().add_message::<AssetLoadFailedEvent<SceneManifestAsset>>()
        .add_message::<AssetEvent<SceneWaterAsset>>().add_message::<AssetLoadFailedEvent<SceneWaterAsset>>()
        .add_message::<AssetEvent<SceneWaterMaskAsset>>().add_message::<AssetLoadFailedEvent<SceneWaterMaskAsset>>()
        .insert_resource(handoff).insert_resource(EditorAuthoringSession::default())
        .insert_resource(SceneEditorState::default())
        .insert_resource(ActiveDynamicScene { scene_id: Some(map.clone()) })
        .insert_resource(feathrise_scene::DynamicWaterStatus {
            scene_id: Some(map.clone()), phase: DynamicWaterPhase::Ready, input_draft_revision: Some(99),
            desired_generation: 12, applied_generation: 12, ..default()
        }).init_resource::<ScenePlacementRuntimeStatus>().init_resource::<FrameCount>()
        .add_systems(Update, poll_water_file_handoff);
    app.update();
    assert!(app.world().resource::<WaterFileHandoff>().pending.as_ref().unwrap().root_pending);
    app.world_mut().resource_mut::<feathrise_scene::DynamicWaterStatus>().input_draft_revision = None;
    app.update();
    let handoff = app.world().resource::<WaterFileHandoff>();
    assert!(!handoff.pending.as_ref().unwrap().root_pending);
    assert!(handoff.pending_for(&map, 19, 5));
    assert_eq!(handoff.formal_water_revision_for_canvas(&map, None), None);
    assert_eq!(handoff.formal_water_revision_for_canvas(&map, Some(&id)), Some(19));
    {
        let mut handoff = app.world_mut().resource_mut::<WaterFileHandoff>();
        let pending = handoff.pending.as_mut().unwrap();
        pending.terrains.as_mut().unwrap().armed = true;
        pending.water = Some(pending_asset());
        pending.gate.all_inputs_succeeded_frame = Some(u32::MAX);
    }
    app.update(); // 仅推进私有目标，不能在根Ready那一帧把它一并跳过。
    assert!(app.world().resource::<WaterFileHandoff>().pending.is_some());
    app.update();
    assert!(!app.world().resource::<WaterFileHandoff>().pending_for(&map, 19, 5));
}
