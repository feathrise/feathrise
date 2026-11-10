//! 后台删除准备的共享写门、精确修订与旧排队票据回归。

use super::*;
use bevy::ecs::system::RunSystemOnce;
use crate::ui::placement_water::{register_placement_instance_root, unregister_placement_instance_root};

fn fixture() -> (App, SceneId) {
    let draft = super::super::test_heightfield_draft(Arc::new(vec![0; 121]));
    let map = draft.scene_id.clone();
    let mut app = App::new();
    app.insert_resource(EditorAuthoringSession { draft: Some(draft), ..default() })
        .init_resource::<PlacementInstanceIndex>()
        .init_resource::<super::super::AuthoringRetirement>()
        .add_observer(register_placement_instance_root)
        .add_observer(unregister_placement_instance_root);
    (app, map)
}

fn mark(app: &mut App, map: &SceneId, id: &str) {
    let (map, id) = (map.clone(), id.to_owned());
    app.world_mut().run_system_once(move |mut commands: Commands, mut session: ResMut<EditorAuthoringSession>| {
        mark_deleted(&mut commands, &mut session, &map, &id);
    }).unwrap();
}

fn record() -> feathrise_scene_content::ScenePlacementRecord {
    feathrise_scene_content::ScenePlacementRecord {
        id: "deleted".into(), asset_path: "gltf/test.glb".into(), asset_target: default(),
        building_preset_id: None, courtyard: None, translation: [0.; 3],
        height_mode: feathrise_scene_content::ScenePlacementHeightMode::FixedHeight, terrain_offset: 0.,
        yaw_degrees: 0., scale: [1.; 3], water_surfaces: Vec::new(), mesh_lod: None,
        terrain_binding: None, editor: default(), terrain_patch: None,
    }
}

fn formal(app: &mut App, map: &SceneId) -> Entity {
    let record = record();
    let root = app.world_mut().spawn((Visibility::Visible, Transform::IDENTITY, ScenePlacementInstance::new(map, &record))).id();
    app.world_mut().flush();
    root
}

#[test]
fn deletion_source_shares_only_until_prepare_or_snapshot_finishes() {
    let intents = PlacementDeletionIntents::default();
    let source = intents.source();
    assert!(intents.write_shared());
    let prepared = source.prepare(["deleted"].into_iter(), std::iter::empty()).unwrap();
    assert!(!intents.write_shared(), "完成值不能继续强共享源表");
    assert!(prepared.matches(&intents));
    let source = intents.source();
    assert!(intents.write_shared());
    let mut snapshot = source.snapshot();
    assert!(!intents.write_shared());
    assert!(snapshot.matches(&intents));
    assert!(snapshot.take().is_empty());
}

#[test]
fn deletion_ack_waits_for_shared_source_then_invalidates_prepared_revision() {
    let (mut app, map) = fixture();
    mark(&mut app, &map, "deleted");
    let session = app.world().resource::<EditorAuthoringSession>();
    let author_revision = session.revision;
    let deletion_revision = session.placement_deletions.revision();
    let source = session.placement_deletions.source();
    let saved = session.placement_deletions.snapshots().remove(0);
    assert!(!acknowledge_saved_deletion(app.world_mut(), &map, &saved));
    assert_eq!(app.world().resource::<EditorAuthoringSession>().placement_deletions.revision(), deletion_revision);
    let prepared = source.prepare(std::iter::empty(), std::iter::empty()).unwrap();
    assert!(acknowledge_saved_deletion(app.world_mut(), &map, &saved));
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.revision, author_revision, "保存确认不能伪造作者修改");
    assert_eq!(session.placement_deletions.revision(), deletion_revision.wrapping_add(1));
    assert!(!prepared.matches(&session.placement_deletions));
    assert!(session.placement_deletions.is_empty());
    assert!(acknowledge_saved_deletion(app.world_mut(), &map, &saved));
    assert_eq!(app.world().resource::<EditorAuthoringSession>().placement_deletions.revision(), deletion_revision.wrapping_add(1),
        "过期确认已经消费但不能再次推进删除修订");
}

#[test]
fn deletion_candidate_kept_old_ticket_cannot_claim_an_undone_queued_mark() {
    let (mut app, map) = fixture();
    let root = formal(&mut app, &map);
    app.world_mut().run_system_once(move |mut commands: Commands, mut session: ResMut<EditorAuthoringSession>| {
        mark_deleted(&mut commands, &mut session, &map, "deleted");
        let prepared = session.placement_deletions.source().prepare(std::iter::empty(), std::iter::empty()).unwrap();
        commands.queue(move |world: &mut World| {
            // 在后续restore世界命令执行前检查，避免“先错遮、再释放”掩盖旧命令冒认。
            assert_eq!(world.get::<Visibility>(root), Some(&Visibility::Visible));
            assert!(world.get::<PlacementDeletionSuppression>(root).is_none());
            drop(prepared);
        });
        restore_deleted(&mut commands, &mut session, &map, "deleted");
    }).unwrap();
}

#[test]
fn deletion_retired_empty_table_cannot_authorize_an_obsolete_queued_restore() {
    let (mut app, map) = fixture();
    let root = formal(&mut app, &map);
    mark(&mut app, &map, "deleted");
    app.world_mut().run_system_once(move |mut commands: Commands, mut session: ResMut<EditorAuthoringSession>| {
        restore_deleted(&mut commands, &mut session, &map, "deleted");
        let prepared = session.placement_deletions.source().prepare(["deleted"].into_iter(), std::iter::empty()).unwrap();
        commands.queue(move |world: &mut World| {
            assert_eq!(world.get::<Visibility>(root), Some(&Visibility::Hidden),
                "当前活动表已重新删除时，旧空表不得授权恢复；不能靠后续视觉队列掩盖错误释放");
            assert!(world.get::<PlacementDeletionSuppression>(root).is_some());
        });
        apply_prepared(&mut commands, &mut session, &map, prepared);
    }).unwrap();
}

#[test]
fn deletion_queued_old_index_cannot_misclassify_add_between_prepared_publications() {
    for deleted in [true, false] {
        let (mut app, map) = fixture();
        app.world_mut().run_system_once(move |mut commands: Commands, mut session: ResMut<EditorAuthoringSession>| {
            if deleted { restore_deleted(&mut commands, &mut session, &map, "deleted"); }
            else { mark_deleted(&mut commands, &mut session, &map, "deleted"); }
            let (deleted_ids, restored_ids) = if deleted { (Some("deleted"), None) } else { (None, Some("deleted")) };
            let prepared = session.placement_deletions.source()
                .prepare(deleted_ids.into_iter(), restored_ids.into_iter()).unwrap();
            let add_scene = map.clone();
            commands.queue(move |world: &mut World| {
                // 这条真实MPI Add位于旧发布之后、新表发布之前；旧表此时仍由退休命令强持。
                let root = world.spawn((Visibility::Visible, Transform::IDENTITY,
                    ScenePlacementInstance::new(&add_scene, &record()))).id();
                world.flush(); // 让真实根Add observer的删除遮蔽命令在本次观察前完成。
                assert_eq!(world.get::<Visibility>(root),
                    Some(if deleted { &Visibility::Hidden } else { &Visibility::Visible }),
                    "退休表不能让迟到Add漏遮，也不能误遮已经恢复的对象");
                assert_eq!(world.get::<PlacementDeletionSuppression>(root).is_some(), deleted);
                let session = world.resource::<EditorAuthoringSession>();
                let document = &world.resource::<PlacementDeletionIndex>().documents[&add_scene];
                assert!(document.ids.ptr_eq(&Arc::downgrade(&session.placement_deletions.ids)),
                    "每条活动文档发布命令均须登记当前表，不能等待最后一条命令修正旧表");
            });
            apply_prepared(&mut commands, &mut session, &map, prepared);
        }).unwrap();
    }
}

#[test]
fn deletion_prepared_adoption_replaces_atomically_and_rejects_other_scope() {
    let (mut app, map) = fixture();
    mark(&mut app, &map, "old");
    let prepared = app.world().resource::<EditorAuthoringSession>().placement_deletions.source()
        .prepare(["new"].into_iter(), ["old"].into_iter()).unwrap();
    assert!(!prepared.matches(&PlacementDeletionIntents::default()));
    let before = app.world().resource::<EditorAuthoringSession>().placement_deletions.revision();
    let mut prepared = Some(prepared);
    app.world_mut().run_system_once(move |mut commands: Commands, mut session: ResMut<EditorAuthoringSession>| {
        apply_prepared(&mut commands, &mut session, &map, prepared.take().expect("单次采用准备结果"));
    }).unwrap();
    let intents = &app.world().resource::<EditorAuthoringSession>().placement_deletions;
    assert_eq!(intents.len(), 1);
    assert_eq!(intents.snapshots()[0].id, "new");
    assert_eq!(intents.revision(), before.wrapping_add(1));
    assert!(!intents.write_shared());
}

#[test]
fn deletion_worker_limit_failure_leaves_source_unchanged_and_writable() {
    let intents = PlacementDeletionIntents::default();
    let ids: Vec<String> = (0..=feathrise_scene_content::MAX_SCENE_PLACEMENTS_PER_SCENE).map(|n| format!("{n}")).collect();
    assert!(intents.source().prepare(ids.iter().map(String::as_str), std::iter::empty()).is_err());
    assert!(intents.is_empty());
    assert_eq!(intents.revision(), 0);
    assert!(!intents.write_shared());
}

#[test]
fn deletion_withdrawal_revokes_ready_results_without_retaining_document_scope() {
    let (mut app, map) = fixture();
    mark(&mut app, &map, "deleted");
    let intents = &app.world().resource::<EditorAuthoringSession>().placement_deletions;
    let identity = intents.identity();
    let mut snapshot = intents.source().snapshot();
    let prepared = intents.source().prepare(std::iter::empty(), std::iter::empty()).unwrap();
    let saved = snapshot.take().remove(0);
    let before = intents.revision();
    assert_eq!(identity.scope.strong_count(), 1, "后台结果和身份均不得强持文档scope");
    let command_map = map.clone();
    app.world_mut().run_system_once(move |mut commands: Commands, session: Res<EditorAuthoringSession>| {
        withdraw_document(&mut commands, &command_map, &session.placement_deletions);
    }).unwrap();
    let intents = &app.world().resource::<EditorAuthoringSession>().placement_deletions;
    assert!(!identity.matches(intents));
    assert!(!snapshot.matches(intents));
    assert!(!prepared.matches(intents));
    assert!(acknowledge_saved_deletion(app.world_mut(), &map, &saved));
    assert_eq!(app.world().resource::<EditorAuthoringSession>().placement_deletions.revision(), before);
    app.insert_resource(EditorAuthoringSession::default());
    assert_eq!(saved.scope.strong_count(), 0, "候选可强持旧ticket，但不得延长已撤回文档生命");
    assert!(saved.ticket.strong_count() > 0);
    assert!(!prepared.matches(&app.world().resource::<EditorAuthoringSession>().placement_deletions));
}
