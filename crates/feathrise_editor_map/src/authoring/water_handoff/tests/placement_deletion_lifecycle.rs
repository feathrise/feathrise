//! 删除票据沿真实保存交接系统采用；不依赖预览Entity存在，也不允许旧保存确认后来意图。

use std::{sync::Arc, time::Duration};

use bevy::{asset::{AssetEvent, AssetLoadFailedEvent, AssetPlugin}, ecs::system::RunSystemOnce, prelude::*};
use feathrise_map_content::{ActiveDynamicMap, MapId, MapManifestAsset, MapWaterAsset, MapWaterMaskAsset};
use feathrise_scene::{DynamicWaterStatus, MapPlacementRuntimePhase, MapPlacementRuntimeStatus, TerrainWaterRebuildRequest};

use super::super::{PlacementDeletionSnapshot, WaterFileHandoff, cleanup_placement_handoff_previews, poll_water_file_handoff};
use crate::{authoring::{EditorAuthoringSession, placement_deletion::{mark_deleted, restore_deleted}}, resources::MapEditorState};

fn deletion_app() -> (App, MapId) {
    let map = MapId::parse("MAP-deletion-save-lifecycle").unwrap();
    let mut draft = crate::authoring::terrain::tests::test_heightfield_draft(Arc::new(vec![0; 121]));
    draft.map_id = map.clone();
    draft.placements.map_id = map.clone();
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .add_message::<TerrainWaterRebuildRequest>()
        .add_message::<AssetEvent<MapManifestAsset>>()
        .add_message::<AssetLoadFailedEvent<MapManifestAsset>>()
        .add_message::<AssetEvent<MapWaterAsset>>()
        .add_message::<AssetLoadFailedEvent<MapWaterAsset>>()
        .add_message::<AssetEvent<MapWaterMaskAsset>>()
        .add_message::<AssetLoadFailedEvent<MapWaterMaskAsset>>()
        .insert_resource(EditorAuthoringSession { draft: Some(draft), revision: 19, ..default() })
        .insert_resource(MapEditorState { open: true, ..default() })
        .insert_resource(ActiveDynamicMap { map_id: Some(map.clone()) })
        .init_resource::<WaterFileHandoff>()
        .init_resource::<DynamicWaterStatus>()
        .init_resource::<bevy::diagnostic::FrameCount>()
        .insert_resource(MapPlacementRuntimeStatus {
            map_id: Some(map.clone()), phase: MapPlacementRuntimePhase::Ready,
            revision: 7, attempt_revision: 11, ..default()
        })
        .add_systems(Update, (poll_water_file_handoff, cleanup_placement_handoff_previews).chain());
    (app, map)
}

fn delete_ids(app: &mut App, ids: Vec<String>) {
    app.world_mut().run_system_once(move |mut commands: Commands, mut session: ResMut<EditorAuthoringSession>| {
        let map = session.draft.as_ref().unwrap().map_id.clone();
        for id in &ids { mark_deleted(&mut commands, &mut session, &map, id); }
    }).unwrap();
}

fn restore_id(app: &mut App, id: &str) {
    let id = id.to_owned();
    app.world_mut().run_system_once(move |mut commands: Commands, mut session: ResMut<EditorAuthoringSession>| {
        let map = session.draft.as_ref().unwrap().map_id.clone();
        restore_deleted(&mut commands, &mut session, &map, &id);
    }).unwrap();
}

fn snapshots(app: &App) -> Vec<PlacementDeletionSnapshot> {
    app.world().resource::<EditorAuthoringSession>().placement_deletions.snapshots()
}

fn begin_saved_delete(app: &mut App, map: &MapId, mut deleted: Vec<PlacementDeletionSnapshot>) {
    let status = app.world().resource::<MapPlacementRuntimeStatus>();
    let (revision, attempt) = (status.revision, status.attempt_revision);
    app.world_mut().resource_mut::<WaterFileHandoff>().begin_placement(
        map.clone(), 19, revision, attempt, 0, &mut Vec::new(), &mut deleted, Duration::ZERO,
    ).unwrap();
    assert!(deleted.is_empty(), "只有完整建门才接管冻结删除快照");
}

fn runtime_status(app: &mut App, phase: MapPlacementRuntimePhase, revision: u64, attempt: u64) {
    let mut status = app.world_mut().resource_mut::<MapPlacementRuntimeStatus>();
    status.phase = phase;
    status.revision = revision;
    status.attempt_revision = attempt;
}

#[test]
fn deletion_only_ready_confirms_at_most_sixty_four_tickets_per_frame() {
    let (mut app, map) = deletion_app();
    delete_ids(&mut app, (0..65).map(|index| format!("deleted-{index:02}")).collect());
    let saved = snapshots(&app);
    begin_saved_delete(&mut app, &map, saved);

    app.update();
    assert_eq!(snapshots(&app).len(), 65, "旧Ready不确认任何删除");
    assert!(!app.world().resource::<WaterFileHandoff>().placement_preview_cleanup_pending());

    runtime_status(&mut app, MapPlacementRuntimePhase::Ready, 8, 12);
    app.update();
    assert_eq!(snapshots(&app).len(), 1, "纯删除批次也必须遵守64项确认预算");
    assert!(app.world().resource::<WaterFileHandoff>().placement_preview_cleanup_pending());

    app.update();
    assert!(snapshots(&app).is_empty());
    assert!(!app.world().resource::<WaterFileHandoff>().placement_preview_cleanup_pending());
}

#[test]
fn deletion_confirmation_keeps_its_cursor_while_a_background_reader_owns_the_table() {
    let (mut app, map) = deletion_app();
    delete_ids(&mut app, (0..65).map(|index| format!("reader-deleted-{index:02}")).collect());
    let saved = snapshots(&app);
    begin_saved_delete(&mut app, &map, saved);
    let (reader, author_revision, deletion_revision) = {
        let session = app.world().resource::<EditorAuthoringSession>();
        (session.placement_deletions.source(), session.revision, session.placement_deletions.revision())
    };
    runtime_status(&mut app, MapPlacementRuntimePhase::Ready, 8, 12);
    app.update();
    assert_eq!(snapshots(&app).len(), 65, "共享读侧未释放时不能消费任何确认项或触发COW");
    let handoff = app.world().resource::<WaterFileHandoff>();
    assert_eq!(handoff.preview_cleanup.as_ref().unwrap().deletion_cursor, 0);
    assert_eq!(app.world().resource::<EditorAuthoringSession>().placement_deletions.revision(), deletion_revision);

    drop(reader); // 模拟worker在返回完整结果之前释放源表，原确认槽才能继续。
    app.update();
    assert_eq!(snapshots(&app).len(), 1, "读侧归还后仍遵守每帧64项额度");
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.revision, author_revision, "保存确认不冒充新的作者编辑");
    assert_ne!(session.placement_deletions.revision(), deletion_revision, "独立删除版本必须使旧后台候选失效");
    app.update();
    assert!(snapshots(&app).is_empty());
    assert!(!app.world().resource::<WaterFileHandoff>().placement_preview_cleanup_pending());
}

#[test]
fn older_saved_delete_cannot_confirm_a_later_delete_of_the_same_id() {
    let (mut app, map) = deletion_app();
    delete_ids(&mut app, vec!["same-id".into()]);
    let first_save = snapshots(&app);
    begin_saved_delete(&mut app, &map, first_save);
    restore_id(&mut app, "same-id");
    delete_ids(&mut app, vec!["same-id".into()]);
    let second_save = snapshots(&app);

    runtime_status(&mut app, MapPlacementRuntimePhase::Ready, 8, 12);
    app.update();
    assert_eq!(snapshots(&app).len(), 1, "S1严格Ready不能清除恢复后重新删除产生的S2票据");

    begin_saved_delete(&mut app, &map, second_save);
    runtime_status(&mut app, MapPlacementRuntimePhase::Ready, 9, 13);
    app.update();
    assert!(snapshots(&app).is_empty(), "只有S2自己的严格新Ready可以确认该票");
}

#[test]
fn failed_delete_is_preserved_until_its_late_strict_ready() {
    let (mut app, map) = deletion_app();
    delete_ids(&mut app, vec!["late-id".into()]);
    let saved = snapshots(&app);
    begin_saved_delete(&mut app, &map, saved);
    runtime_status(&mut app, MapPlacementRuntimePhase::Failed, 7, 12);
    app.update();
    assert_eq!(snapshots(&app).len(), 1);
    assert!(app.world().resource::<WaterFileHandoff>().placement_retry_required_for(&map));
    assert!(!app.world().resource::<WaterFileHandoff>().placement_preview_cleanup_pending());

    runtime_status(&mut app, MapPlacementRuntimePhase::Ready, 8, 12);
    app.update();
    assert!(snapshots(&app).is_empty(), "迟到Ready仍须携带原冻结票据进入有界确认槽");
}

#[test]
fn leaving_map_never_confirms_delete_from_a_lingering_old_map_ready() {
    for failed_first in [false, true] {
        let (mut app, map) = deletion_app();
        delete_ids(&mut app, vec!["resident-id".into()]);
        let saved = snapshots(&app);
        begin_saved_delete(&mut app, &map, saved);
        if failed_first {
            runtime_status(&mut app, MapPlacementRuntimePhase::Failed, 7, 12);
            app.update(); // 同时覆盖未解决门的late_placement分支。
        }
        app.world_mut().resource_mut::<ActiveDynamicMap>().map_id =
            Some(MapId::parse("MAP-deletion-save-other").unwrap());
        runtime_status(&mut app, MapPlacementRuntimePhase::Ready, 8, 12);
        app.update();
        assert_eq!(snapshots(&app).len(), 1, "离图只允许原门放行，不能确认驻留文档删除");
        assert!(!app.world().resource::<WaterFileHandoff>().placement_preview_cleanup_pending());
    }
}
