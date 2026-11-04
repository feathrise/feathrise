//! 高度/水沿同一完整快照ID信用，不能在首个地形建立前占用唯一实时正文槽。
use super::*;
use crate::authoring::terrain_documents::TerrainDocumentIdentity;
use feathrise_scene::TerrainObjectAuthoringTarget;

#[test]
fn embedded_preview_new_terrain_credit_gates_height_and_water_until_matching_snapshot_ack() {
    let (mut app, map) = setup();
    let object = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        let object = draft.manifest.terrain_objects_for_edit()[0].clone();
        draft.manifest.terrains = Some(Arc::from([object.clone()]));
        draft.terrain_documents.active = Some(TerrainDocumentIdentity { manifest: object.source_manifest(&draft.manifest),
            folder: object.private_folder(&map), definition: Arc::new(object.clone()) });
        object
    };
    feathrise_scene::runtime_test_support::install_active_terrain_instances(app.world_mut(), &map, &[]);
    app.init_resource::<TerrainObjectAuthoringTarget>();
    {
        let mut target = app.world_mut().resource_mut::<TerrainObjectAuthoringTarget>();
        target.map = Some(map.clone()); target.terrain = Some(object.id.clone());
    }
    feathrise_scene::runtime_test_support::apply_authoring_target(app.world_mut());
    app.world_mut().resource_mut::<SparseHeightLiveOverride>().retarget(Some(map.clone()), 1);
    app.world_mut().resource_mut::<DynamicClipmapWaterDraft>().publish(DynamicClipmapWaterDraftSnapshot {
        source_revision: 5, definition: feathrise_map_content::MapWaterAsset { map_id: map.clone(), ..default() },
        sources: default(), masks: default(),
    });
    let mut state = app.world_mut().remove_resource::<EmbeddedAuthorPreviewState>().unwrap();
    assert!(super::super::source::capture(app.world(), &mut state, &map).is_none());
    app.insert_resource(state);
    snapshot_sent(app.world_mut(), 7, 1, 2, map.clone(), Arc::new(crate::standalone::embedded_snapshot::EmbeddedPoseTargets {
        sub_maps: default(), terrains: [object.id.clone()].into_iter().collect(),
    }));
    assert!(!snapshot_applied(app.world_mut(), 7, 2, 2));
    assert!(snapshot_applied(app.world_mut(), 7, 1, 2));
    let mut state = app.world_mut().remove_resource::<EmbeddedAuthorPreviewState>().unwrap();
    state.cursor = 1;
    assert!(matches!(super::super::source::capture(app.world(), &mut state, &map),
        Some((super::super::resources::Key::Height(Some(id)), _, _)) if id == object.id));
    state.cursor = 2;
    assert!(matches!(super::super::source::capture(app.world(), &mut state, &map),
        Some((super::super::resources::Key::Water(Some(id), false), _, _)) if id == object.id));
}
