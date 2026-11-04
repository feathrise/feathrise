use super::*;
use crate::standalone::{
    embedded_author_preview as bridge, embedded_snapshot::EmbeddedPoseTargets,
};
use feathrise_map_content::{
    MapInstanceTransform,
    embedded_author_preview::{decode, encode},
};

fn reference(id: &str, x: f32) -> MapInstancePlacement {
    MapInstancePlacement {
        id: MapInstanceId::parse(id).unwrap(),
        source_map_id: MapId::parse("source").unwrap(),
        label: String::new(),
        transform: MapInstanceTransform {
            translation: [x, 0., 0.],
            ..default()
        },
    }
}
fn pose(id: &str, x: f32, active: bool) -> FrozenPose {
    FrozenPose::SubMap {
        id: MapInstanceId::parse(id).unwrap(),
        source: Arc::from([reference(id, x)]),
        active,
    }
}

#[test]
fn embedded_object_pose_reliable_end_survives_update_coalescing_and_target_switch() {
    let mut mailbox = PoseMailbox::default();
    assert!(mailbox.push(pose("first", 1., true)));
    let first = mailbox.queue.front().unwrap().0;
    assert!(mailbox.push(pose("first", 2., true)));
    assert!(mailbox.push(pose("first", 3., true)));
    assert_eq!(mailbox.queue.len(), 1);
    mailbox.acknowledge(first);
    assert_eq!(
        mailbox.queue.len(),
        1,
        "迟到的旧 Update ACK 不能吞掉合并后的新姿态"
    );
    assert!(mailbox.push(pose("first", 0., false)));
    assert!(mailbox.push(pose("second", 9., true)));
    assert_eq!(mailbox.queue.len(), 3);
    let end = &mailbox.queue[1];
    let value =
        decode(&encode(&end.1.clone().encode(MapId::parse("owner").unwrap(), end.0)).unwrap())
            .unwrap();
    assert!(
        matches!(value, AuthorPreview::SubMapPose { id, pose:Some(pose), active:false, .. }
        if id.as_str()=="first" && pose.translation[0]==0.)
    );
    assert!(mailbox.queue[0].0 < mailbox.queue[1].0 && mailbox.queue[1].0 < mailbox.queue[2].0);
}

#[test]
fn embedded_object_pose_snapshot_credit_rejects_old_ack_and_new_id_waits_without_occupying_body() {
    let mut world = World::new();
    world.init_resource::<crate::authoring::AuthoringRetirement>();
    world.init_resource::<EmbeddedAuthorPreviewState>();
    let draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    let map = draft.map_id.clone();
    world.insert_resource(EditorAuthoringSession {
        draft: Some(draft),
        ..default()
    });
    let id = TerrainId::parse("new-terrain").unwrap();
    world.insert_resource(TerrainObjectTransformPreview::default());
    world.resource_mut::<TerrainObjectTransformPreview>().pose =
        Some((map.clone(), id.clone(), TerrainObjectTransform::default()));
    {
        let mut state = world.resource_mut::<EmbeddedAuthorPreviewState>();
        state.enabled = true;
        state.session = 9;
        state.generation = 2;
    }
    let targets = Arc::new(EmbeddedPoseTargets {
        terrains: [id].into_iter().collect(),
        ..default()
    });
    bridge::snapshot_sent(&mut world, 9, 2, 20, map.clone(), targets.clone());
    assert!(!bridge::snapshot_applied(&mut world, 8, 2, 20));
    assert!(!bridge::snapshot_applied(&mut world, 9, 1, 20));
    assert!(!bridge::snapshot_applied(&mut world, 9, 2, 19));
    let mut state = world
        .remove_resource::<EmbeddedAuthorPreviewState>()
        .unwrap();
    observe(&world, &mut state, &map);
    assert!(
        state.terrain_pose.queue.is_empty(),
        "新 ID 尚未真实采用，不能挡完整快照正文槽"
    );
    world.insert_resource(state);
    assert!(bridge::snapshot_applied(&mut world, 9, 2, 20));
    let mut state = world
        .remove_resource::<EmbeddedAuthorPreviewState>()
        .unwrap();
    observe(&world, &mut state, &map);
    assert_eq!(
        state.terrain_pose.queue.len(),
        1,
        "真实回执后在同一笔手势内恢复实时投影"
    );
    assert!(Arc::ptr_eq(
        &state.applied_pose_targets.as_ref().unwrap().targets,
        &targets
    ));
    state.session = 10;
    retire_stale(&mut world, &mut state);
    assert!(state.applied_pose_targets.is_none());
    world.insert_resource(state);
    assert!(!bridge::snapshot_applied(&mut world, 9, 2, 20));
}

#[test]
fn embedded_object_pose_snapshot_invalidation_discards_old_end_and_keeps_monotonic_ticket() {
    let mut mailbox = PoseMailbox::default();
    mailbox.push(pose("first", 1., true));
    mailbox.push(pose("first", 2., false));
    let old = mailbox.ticket;
    mailbox.clear();
    mailbox.push(pose("first", 3., true));
    assert!(mailbox.ticket > old);
    mailbox.acknowledge(old);
    assert_eq!(mailbox.queue.len(), 1);
    let (ticket, value) = mailbox.queue.front().unwrap();
    assert!(
        matches!(value.clone().encode(MapId::parse("owner").unwrap(),*ticket),
        AuthorPreview::SubMapPose {pose:Some(pose),..} if pose.translation[0]==3.)
    );
}
