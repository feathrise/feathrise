//! 本地已存在的地形仍须先让完整快照建立游戏来源，避免预览与首个根互等。
use super::*;
use crate::standalone::{embedded_author_preview as author, embedded_snapshot::EmbeddedPoseTargets};

pub(super) fn terrain_credit(app: &mut App, map: &SceneId, objects: &[feathrise_scene_content::SceneTerrainObject]) {
    app.init_resource::<author::EmbeddedAuthorPreviewState>();
    app.world_mut().resource_mut::<author::EmbeddedAuthorPreviewState>().enabled = true;
    author::snapshot_sent(app.world_mut(), 7, 1, 1, map.clone(), Arc::new(EmbeddedPoseTargets {
        subscenes: default(), terrains: objects.iter().map(|value| value.id.clone()).collect(),
    }));
    assert!(author::snapshot_applied(app.world_mut(), 7, 1, 1));
}

#[test]
fn embedded_preview_new_terrain_credit_prevents_vegetation_from_blocking_first_snapshot() {
    use feathrise_scene_core::residency::InstanceResourceTable;
    let (mut app, map) = setup();
    let (objects, roots) = explicit_canvases(&mut app, &map, true);
    terrain_credit(&mut app, &map, &[]); // 游戏已回执空场景，但本地先完成两个新画布。
    publish(app.world_mut(), &map, 7);
    app.world_mut().resource_mut::<InstanceResourceTable<DynamicBladeDraft>>().values.get_mut(&roots[1]).unwrap()
        .publish(feathrise_scene::DynamicBladeDraftSnapshot { source_revision: 8,
            definition: SceneVegetationAsset { model_surfaces: Vec::new().into(), scene_id: map.clone(), ..default() }, sources: default(),
            densities: default(), nonzero_layers: default() });
    for _ in 0..8 { app.update(); }
    assert!(app.world().resource::<EmbeddedVegetationState>().ready.is_none());
    assert!(!app.world().resource::<EmbeddedVegetationState>().pending, "不给未知根建立会挡住完整快照的正文");
    author::snapshot_sent(app.world_mut(), 7, 1, 2, map.clone(), Arc::new(EmbeddedPoseTargets {
        subscenes: default(), terrains: objects.iter().map(|value| value.id.clone()).collect(),
    }));
    assert!(!author::snapshot_applied(app.world_mut(), 8, 1, 2));
    for _ in 0..4 { app.update(); }
    assert!(app.world().resource::<EmbeddedVegetationState>().ready.is_none(), "发送或旧会话ACK都不是采用信用");
    assert!(author::snapshot_applied(app.world_mut(), 7, 1, 2));
    until(&mut app, |world| world.resource::<EmbeddedVegetationState>().ready.is_some());
    let state = app.world().resource::<EmbeddedVegetationState>();
    let batch = decode_vegetation_preview(&state.ready.as_ref().unwrap().bytes).unwrap();
    assert_eq!(batch.canvases.len(), 2, "先前未发送的根/兄弟草稿都必须在正确ACK后补发");
    assert!(app.world().resource::<VegetationBrushStroke>().active);
}
