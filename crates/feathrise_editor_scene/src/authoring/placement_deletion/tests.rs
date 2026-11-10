//! 删除票据在排队命令、文档所有权转移和精确保存确认间的生命周期回归。

use super::*;
use bevy::ecs::system::RunSystemOnce;
use crate::ui::placement_water::{register_placement_instance_root, unregister_placement_instance_root};

fn fixture() -> (App, SceneId, feathrise_scene_content::ScenePlacementRecord) {
    let mut draft = super::super::test_heightfield_draft(Arc::new(vec![0; 121]));
    let map = draft.scene_id.clone();
    let record = feathrise_scene_content::ScenePlacementRecord {
        id: "deleted".to_owned(), asset_path: "gltf/test.glb".to_owned(), asset_target: Default::default(),
        building_preset_id: None, courtyard: None, translation: [0.; 3],
        height_mode: feathrise_scene_content::ScenePlacementHeightMode::FixedHeight, terrain_offset: 0.,
        yaw_degrees: 0., scale: [1.; 3], water_surfaces: Vec::new(), mesh_lod: None,
        terrain_binding: None, editor: Default::default(), terrain_patch: None,
    };
    draft.placements.placements.clear();
    let mut app = App::new();
    app.insert_resource(EditorAuthoringSession { draft: Some(draft), ..default() })
        .init_resource::<PlacementInstanceIndex>()
        .add_observer(register_placement_instance_root)
        .add_observer(unregister_placement_instance_root);
    (app, map, record)
}

fn delete(app: &mut App, map: &SceneId, id: &str) {
    let (map, id) = (map.clone(), id.to_owned());
    app.world_mut().run_system_once(move |mut commands: Commands, mut session: ResMut<EditorAuthoringSession>| {
        mark_deleted(&mut commands, &mut session, &map, &id);
    }).unwrap();
}

fn restore(app: &mut App, map: &SceneId, id: &str) {
    let (map, id) = (map.clone(), id.to_owned());
    app.world_mut().run_system_once(move |mut commands: Commands, mut session: ResMut<EditorAuthoringSession>| {
        restore_deleted(&mut commands, &mut session, &map, &id);
    }).unwrap();
}

fn formal(app: &mut App, map: &SceneId, record: &feathrise_scene_content::ScenePlacementRecord) -> Entity {
    let root = app.world_mut().spawn((Visibility::Visible, Transform::IDENTITY, ScenePlacementInstance::new(map, record))).id();
    app.world_mut().flush();
    root
}

#[test]
fn deletion_batched_weak_table_updates_mask_every_existing_root() {
    let (mut app, map, record) = fixture();
    let first = formal(&mut app, &map, &record);
    let mut second_record = record.clone(); second_record.id = "second".into();
    let second = formal(&mut app, &map, &second_record);
    let command_map = map.clone();
    app.world_mut().run_system_once(move |mut commands: Commands, mut session: ResMut<EditorAuthoringSession>| {
        mark_deleted(&mut commands, &mut session, &command_map, "deleted");
        mark_deleted(&mut commands, &mut session, &command_map, "second");
    }).unwrap();
    for root in [first, second] {
        assert!(suppression_is_current(app.world(), root));
        assert_eq!(app.world().get::<Visibility>(root), Some(&Visibility::Hidden));
    }
    assert_eq!(app.world().resource::<EditorAuthoringSession>().placement_deletions.snapshots().len(), 2);
}

#[test]
fn deletion_warm_session_move_keeps_late_add_covered_without_active_draft() {
    let (mut app, map, record) = fixture();
    delete(&mut app, &map, &record.id);
    let parked = app.world_mut().remove_resource::<EditorAuthoringSession>().unwrap();
    app.insert_resource(EditorAuthoringSession::default());
    let late = formal(&mut app, &map, &record);
    assert_eq!(app.world().get::<Visibility>(late), Some(&Visibility::Hidden));
    assert!(suppression_is_current(app.world(), late));
    app.insert_resource(parked);
    restore(&mut app, &map, &record.id);
    assert_eq!(app.world().get::<Visibility>(late), Some(&Visibility::Visible));
}

#[test]
fn deletion_old_saved_version_cannot_acknowledge_redone_delete() {
    let (mut app, map, record) = fixture();
    delete(&mut app, &map, &record.id);
    let saved = app.world().resource::<EditorAuthoringSession>().placement_deletions.snapshots().remove(0);
    restore(&mut app, &map, &record.id);
    delete(&mut app, &map, &record.id);
    acknowledge_saved_deletion(app.world_mut(), &map, &saved);
    assert_eq!(app.world().resource::<EditorAuthoringSession>().placement_deletions.snapshots().len(), 1);
    let late = formal(&mut app, &map, &record);
    assert_eq!(app.world().get::<Visibility>(late), Some(&Visibility::Hidden));
}

#[test]
fn deletion_exact_acknowledgment_keeps_old_root_mask_but_releases_logical_ticket() {
    let (mut app, map, record) = fixture();
    let old = formal(&mut app, &map, &record);
    delete(&mut app, &map, &record.id);
    let saved = app.world().resource::<EditorAuthoringSession>().placement_deletions.snapshots().remove(0);
    acknowledge_saved_deletion(app.world_mut(), &map, &saved);
    assert!(app.world().resource::<EditorAuthoringSession>().placement_deletions.snapshots().is_empty());
    release_expired_suppression(app.world_mut(), old);
    assert!(suppression_is_current(app.world(), old));
    assert_eq!(app.world().get::<Visibility>(old), Some(&Visibility::Hidden));
    let external_new = formal(&mut app, &map, &record);
    assert_eq!(app.world().get::<Visibility>(external_new), Some(&Visibility::Visible));
    restore(&mut app, &map, &record.id);
    assert_eq!(app.world().get::<Visibility>(old), Some(&Visibility::Visible));
}

#[test]
fn deletion_cold_withdrawal_cannot_remove_a_new_document_of_same_scene() {
    let (mut app, map, record) = fixture();
    delete(&mut app, &map, &record.id);
    let old_set = std::mem::take(&mut app.world_mut().resource_mut::<EditorAuthoringSession>().placement_deletions);
    delete(&mut app, &map, &record.id);
    let command_map = map.clone();
    app.world_mut().run_system_once(move |mut commands: Commands| withdraw_document(&mut commands, &command_map, &old_set)).unwrap();
    let late = formal(&mut app, &map, &record);
    assert!(suppression_is_current(app.world(), late));
    assert_eq!(app.world().get::<Visibility>(late), Some(&Visibility::Hidden));
}

#[test]
fn deletion_saved_snapshot_does_not_keep_discarded_document_alive() {
    let (mut app, map, record) = fixture();
    delete(&mut app, &map, &record.id);
    let saved = app.world().resource::<EditorAuthoringSession>().placement_deletions.snapshots();
    app.insert_resource(EditorAuthoringSession::default());
    let external = formal(&mut app, &map, &record);
    assert_eq!(app.world().get::<Visibility>(external), Some(&Visibility::Visible));
    assert_eq!(saved[0].ticket.strong_count(), 0);
}

#[test]
fn deletion_withdrawal_invalidates_queued_claim_while_old_session_is_still_owned() {
    let (mut app, map, record) = fixture();
    let root = formal(&mut app, &map, &record);
    let command_map = map.clone();
    app.world_mut().run_system_once(move |mut commands: Commands, mut session: ResMut<EditorAuthoringSession>| {
        mark_deleted(&mut commands, &mut session, &command_map, "deleted");
        withdraw_document(&mut commands, &command_map, &session.placement_deletions);
    }).unwrap();
    assert_eq!(app.world().get::<Visibility>(root), Some(&Visibility::Visible));
    assert!(app.world().get::<PlacementDeletionSuppression>(root).is_none());
}

#[test]
fn deletion_expiry_does_not_write_old_scene_visibility_to_reidentified_entity() {
    let (mut app, map, record) = fixture();
    let root = formal(&mut app, &map, &record);
    delete(&mut app, &map, &record.id);
    let other_scene = SceneId::parse("MAP-deletion-foreign-reidentified").unwrap();
    app.world_mut().entity_mut(root).remove::<ScenePlacementInstance>();
    app.world_mut().entity_mut(root).insert((Visibility::Hidden, ScenePlacementInstance::new(&other_scene, &record)));
    app.world_mut().flush();
    release_expired_suppression(app.world_mut(), root);
    assert!(app.world().get::<PlacementDeletionSuppression>(root).is_none());
    assert_eq!(app.world().get::<Visibility>(root), Some(&Visibility::Hidden), "不能将旧场景的Visible写到新的隐藏根");
}
