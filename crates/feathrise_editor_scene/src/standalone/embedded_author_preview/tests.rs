use super::*;
#[path = "target_credit_tests.rs"]
mod target_credit_tests;
use crate::authoring::EditorAuthoringSession;
use feathrise_scene_content::{
    SceneId, SparseHeightLiveOverride, SparseHeightSpec, SparseHeightTile, SparseHeightTileKey,
    embedded_author_preview::{AuthorPreview, decode},
};
use feathrise_scene::{DynamicClipmapWaterDraft, DynamicClipmapWaterDraftSnapshot};
use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant},
};

fn setup() -> (App, SceneId) {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(AuthorPreviewPlugin);
    let draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    let map = draft.scene_id.clone();
    app.insert_resource(EditorAuthoringSession {
        draft: Some(draft),
        ..default()
    });
    app.init_resource::<SparseHeightLiveOverride>()
        .init_resource::<DynamicClipmapWaterDraft>();
    {
        let mut state = app.world_mut().resource_mut::<EmbeddedAuthorPreviewState>();
        state.enabled = true;
        state.session = 7;
        state.generation = 1;
    }
    (app, map)
}
fn ready(app: &mut App, after: u64) -> Arc<EmbeddedAuthorPreview> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        app.update();
        let state = app.world().resource::<EmbeddedAuthorPreviewState>();
        assert!(state.error.is_none(), "{:?}", state.error);
        if let Some(value) = state.ready.as_ref().filter(|value| value.sequence > after) {
            return value.clone();
        }
        assert!(Instant::now() < deadline, "作者实时源未完成");
        std::thread::sleep(Duration::from_millis(1));
    }
}
#[test]
fn embedded_author_preview_height_ack_diff_sends_only_changed_persistent_tile() {
    let (mut app, map) = setup();
    let spec = SparseHeightSpec::new([-512.; 2], [512.; 2], 0.125, -8., 16.).unwrap();
    let keys = [
        SparseHeightTileKey { x: 0, z: 0 },
        SparseHeightTileKey { x: 1, z: 0 },
    ];
    let tiles = keys.map(|key| Arc::new(SparseHeightTile::blank(&spec, key).unwrap()));
    {
        let mut live = app.world_mut().resource_mut::<SparseHeightLiveOverride>();
        live.retarget(Some(map.clone()), 1);
        live.publish_tiles(&map, 2, tiles).unwrap();
    }
    let first = ready(&mut app, 0);
    assert!(
        matches!(decode(&first.bytes).unwrap(),AuthorPreview::Height{reset:true,tiles,..}if tiles.len()==2)
    );
    {
        let mut live = app.world_mut().resource_mut::<SparseHeightLiveOverride>();
        live.publish_tiles(
            &map,
            3,
            [Arc::new(SparseHeightTile::blank(&spec, keys[1]).unwrap())],
        )
        .unwrap();
    }
    for _ in 0..5 {
        app.update();
    }
    assert_eq!(
        app.world()
            .resource::<EmbeddedAuthorPreviewState>()
            .ready
            .as_ref()
            .unwrap()
            .sequence,
        first.sequence
    );
    app.world_mut()
        .resource_mut::<EmbeddedAuthorPreviewState>()
        .acknowledged_sequence = first.sequence;
    let second = ready(&mut app, first.sequence);
    assert!(
        matches!(decode(&second.bytes).unwrap(),AuthorPreview::Height{reset:false,tiles,revision:3,..}if tiles.len()==1&&tiles[0].key==keys[1])
    );
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    );
}
#[test]
fn embedded_author_preview_water_uses_published_arc_and_reconnect_rejects_old_ack() {
    let (mut app, map) = setup();
    app.world_mut()
        .resource_mut::<DynamicClipmapWaterDraft>()
        .publish_root(DynamicClipmapWaterDraftSnapshot {
            source_revision: 42,
            definition: feathrise_scene_content::SceneWaterAsset {
                scene_id: map,
                ..default()
            },
            sources: BTreeMap::new(),
            masks: BTreeMap::new(),
        });
    let first = ready(&mut app, 0);
    assert!(matches!(
        decode(&first.bytes).unwrap(),
        AuthorPreview::Water {
            root: true,
            revision: 42,
            ..
        }
    ));
    {
        let mut state = app.world_mut().resource_mut::<EmbeddedAuthorPreviewState>();
        state.session = 8;
        state.acknowledged_sequence = first.sequence;
    }
    let second = ready(&mut app, first.sequence);
    assert_eq!(second.session, 8);
    assert!(matches!(
        decode(&second.bytes).unwrap(),
        AuthorPreview::Water {
            root: true,
            revision: 42,
            ..
        }
    ));
}

#[test]
fn embedded_author_preview_same_generation_snapshot_invalidation_keeps_sequence_and_rejects_old_ack()
 {
    let (mut app, map) = setup();
    app.world_mut()
        .resource_mut::<DynamicClipmapWaterDraft>()
        .publish_root(DynamicClipmapWaterDraftSnapshot {
            source_revision: 42,
            definition: feathrise_scene_content::SceneWaterAsset {
                scene_id: map,
                ..default()
            },
            sources: BTreeMap::new(),
            masks: BTreeMap::new(),
        });
    let first = ready(&mut app, 0);
    let epoch = app.world().resource::<EmbeddedAuthorPreviewState>().epoch;
    invalidate(app.world_mut());
    {
        let state = app.world().resource::<EmbeddedAuthorPreviewState>();
        assert!(state.ready.is_none(), "完整快照失效后同帧不能再取旧候选");
        assert!(state.offered.is_none());
        assert_eq!(state.sequence, first.sequence);
        assert_eq!((state.session, state.generation), (7, 1));
    }
    // 旧候选的真实迟到 ACK 不能替同一来源重放的新包取得采用信用。
    app.world_mut()
        .resource_mut::<EmbeddedAuthorPreviewState>()
        .acknowledged_sequence = first.sequence;
    let second = ready(&mut app, first.sequence);
    assert_eq!((second.session, second.generation), (7, 1));
    assert!(second.sequence > first.sequence);
    assert!(app.world().resource::<EmbeddedAuthorPreviewState>().epoch > epoch);
    for _ in 0..3 {
        app.update();
    }
    {
        let state = app.world().resource::<EmbeddedAuthorPreviewState>();
        assert_eq!(state.ready.as_ref().unwrap().sequence, second.sequence);
        assert!(state.acknowledged.is_empty());
    }
    app.world_mut()
        .resource_mut::<EmbeddedAuthorPreviewState>()
        .acknowledged_sequence = second.sequence;
    app.update();
    let state = app.world().resource::<EmbeddedAuthorPreviewState>();
    assert!(state.ready.is_none());
    assert!(!state.pending);
    assert!(
        state
            .acknowledged
            .contains_key(&resources::Key::Water(None, true))
    );
}

#[test]
fn embedded_author_preview_snapshot_invalidation_keeps_task_slot_and_rejects_stale_completion() {
    let (mut app, map) = setup();
    let publish = |world: &mut World, revision| {
        world
            .resource_mut::<DynamicClipmapWaterDraft>()
            .publish_root(DynamicClipmapWaterDraftSnapshot {
                source_revision: revision,
                definition: feathrise_scene_content::SceneWaterAsset {
                    scene_id: map.clone(),
                    ..default()
                },
                sources: BTreeMap::new(),
                masks: BTreeMap::new(),
            });
    };
    publish(app.world_mut(), 41);
    app.update();
    let old_sequence = app
        .world()
        .resource::<EmbeddedAuthorPreviewState>()
        .sequence;
    assert!(
        app.world()
            .resource::<EmbeddedAuthorPreviewState>()
            .task
            .is_some()
    );
    invalidate(app.world_mut());
    {
        let state = app.world().resource::<EmbeddedAuthorPreviewState>();
        assert!(state.task.is_some(), "失效不丢弃尚未归还的唯一任务槽");
        assert!(state.pending);
        assert_eq!(state.sequence, old_sequence);
    }
    publish(app.world_mut(), 42);
    let replayed = ready(&mut app, old_sequence);
    assert!(matches!(
        decode(&replayed.bytes).unwrap(),
        AuthorPreview::Water { revision: 42, .. }
    ));
    assert!(replayed.sequence > old_sequence);
}
