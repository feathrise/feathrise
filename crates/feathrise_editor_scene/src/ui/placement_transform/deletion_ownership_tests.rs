//! 沿真实删除按钮与撤销入口验证删除责任，覆盖迟到正式根、跨场景隔离和再次删除。

use super::*;
use feathrise_scene_content::{SceneId, ScenePlacementRecord};

/// 正式根只携带场景身份，不加入作者预览表；Add必须独立承担已经采用的删除意图。
fn late_formal(app: &mut App, map: &SceneId, record: &ScenePlacementRecord) -> (Entity, Entity, Transform) {
    formal_with_instance(app, ScenePlacementInstance::new(map, record))
}

fn formal_with_instance(app: &mut App, instance: ScenePlacementInstance) -> (Entity, Entity, Transform) {
    let parent = app.world_mut().spawn((Transform::IDENTITY, Visibility::Inherited)).id();
    let transform = Transform::from_xyz(25.0, 26.0, 27.0)
        .with_rotation(Quat::from_rotation_y(0.7))
        .with_scale(Vec3::new(1.5, 2.5, 3.5));
    let root = app.world_mut().spawn((instance,
        transform, Visibility::Inherited, ChildOf(parent))).id();
    app.world_mut().flush();
    (root, parent, transform)
}

fn assert_hidden_without_rewriting_formal(app: &App, root: Entity, parent: Entity, transform: Transform) {
    assert_eq!(app.world().get::<Visibility>(root), Some(&Visibility::Hidden),
        "记录删除成功之后到达的旧正式根必须继续隐藏");
    assert_eq!(app.world().get::<Transform>(root), Some(&transform), "隐藏不能改写正式候选的姿态");
    assert_eq!(app.world().get::<ChildOf>(root).map(ChildOf::parent), Some(parent),
        "隐藏不能改变正式候选的层级和物理归属");
}

#[test]
fn deletion_button_rejects_a_shared_table_before_changing_record_preview_or_history() {
    let (mut app, preview, record, map) = deletion_fixture(false);
    let (reader, before_revision) = {
        let session = app.world().resource::<EditorAuthoringSession>();
        (session.placement_deletions.source(), session.revision)
    };
    click_delete(&mut app);
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.draft.as_ref().unwrap().placements.placements, [record.clone()]);
    assert_eq!(session.revision, before_revision);
    assert_eq!(session.placement_previews.get(&record.id), Some(&preview));
    assert!(session.undo.is_empty() && session.redo.is_empty());
    assert!(session.placement_deletions.snapshots().is_empty());
    assert!(app.world().get_entity(preview).is_ok());
    assert!(app.world().resource::<SceneEditorState>().status.contains("后台"));

    drop(reader);
    click_delete(&mut app);
    assert!(app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().placements.placements.is_empty());
    let (late, parent, pose) = late_formal(&mut app, &map, &record);
    assert_hidden_without_rewriting_formal(&app, late, parent, pose);
}

#[test]
fn single_deletion_undo_keeps_the_request_and_stack_until_a_shared_reader_finishes() {
    let (mut app, _, record, _) = deletion_fixture(false);
    app.init_asset::<bevy::world_serialization::WorldAsset>();
    crate::viewport::placement_move::interaction_tests::batch_undo_resources(&mut app);
    click_delete(&mut app);
    let reader = app.world().resource::<EditorAuthoringSession>().placement_deletions.source();
    app.world_mut().resource_mut::<SceneEditorState>().undo_requested = true;
    app.world_mut().run_system_once(crate::authoring::apply_undo_redo).unwrap();
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!((session.undo.len(), session.redo.len()), (1, 0));
    assert!(session.draft.as_ref().unwrap().placements.placements.is_empty());
    assert!(app.world().resource::<SceneEditorState>().undo_requested);

    drop(reader);
    app.world_mut().run_system_once(crate::authoring::apply_undo_redo).unwrap();
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.draft.as_ref().unwrap().placements.placements, [record]);
    assert_eq!((session.undo.len(), session.redo.len()), (0, 1));
    assert!(!app.world().resource::<SceneEditorState>().undo_requested);
}

#[test]
fn successful_delete_masks_formal_roots_added_after_the_author_preview_is_gone() {
    for terrain_patch in [false, true] {
        let (mut app, preview, record, map) = deletion_fixture(terrain_patch);
        click_delete(&mut app);
        let session = app.world().resource::<EditorAuthoringSession>();
        assert!(session.draft.as_ref().unwrap().placements.placements.is_empty());
        assert!(!session.placement_previews.contains_key(&record.id));
        assert_eq!(session.undo.len(), 1);
        assert!(app.world().get_entity(preview).is_err());

        // 没有自有预览可继续遮蔽，旧保存批次后续Add必须由明确删除责任拦住。
        let (late, parent, transform) = late_formal(&mut app, &map, &record);
        assert_hidden_without_rewriting_formal(&app, late, parent, transform);
    }
}

#[test]
fn successful_delete_keeps_late_pending_roots_hidden_through_real_batch_activation() {
    for terrain_patch in [false, true] {
        let (mut app, _, record, map) = deletion_fixture(terrain_patch);
        click_delete(&mut app);
        let (pending, activation) = feathrise_scene::runtime_test_support::pending_placement_instance(&map, &record);
        let (late, parent, transform) = formal_with_instance(&mut app, pending);
        assert!(app.world().get::<ScenePlacementInstance>(late).unwrap().is_pending());
        assert_hidden_without_rewriting_formal(&app, late, parent, transform);

        // 使用正式共享门直接激活，不替换MPI，不给修复伪造第二次Add或Insert机会。
        activation.activate();
        assert!(app.world().get::<ScenePlacementInstance>(late).unwrap().is_active());
        assert_hidden_without_rewriting_formal(&app, late, parent, transform);
    }
}

#[test]
fn successful_delete_keeps_external_neighbor_and_same_id_other_scene_visible() {
    let (mut app, _, record, map) = deletion_fixture(false);
    click_delete(&mut app);
    let mut neighbor = record.clone();
    neighbor.id = "external-late-neighbor".into();
    let other_scene = SceneId::parse("MAP-deletion-late-foreign").unwrap();
    let (neighbor_root, neighbor_parent, neighbor_pose) = late_formal(&mut app, &map, &neighbor);
    let (foreign_root, foreign_parent, foreign_pose) = late_formal(&mut app, &other_scene, &record);

    // 邻ID也不在作者草稿中，防止用“草稿缺记录”冒充删除票据而吞掉外部热更新新增。
    for (root, parent, pose) in [(neighbor_root, neighbor_parent, neighbor_pose),
        (foreign_root, foreign_parent, foreign_pose)] {
        assert_eq!(app.world().get::<Visibility>(root), Some(&Visibility::Inherited));
        assert_eq!(app.world().get::<Transform>(root), Some(&pose));
        assert_eq!(app.world().get::<ChildOf>(root).map(ChildOf::parent), Some(parent));
    }
    assert_eq!(app.world().get::<ScenePlacementInstance>(neighbor_root).unwrap().id(), neighbor.id);
    assert_eq!(app.world().get::<ScenePlacementInstance>(foreign_root).unwrap().scene_id(), &other_scene);
}

#[test]
fn undo_delete_then_delete_again_masks_a_new_late_formal_generation() {
    let (mut app, _, record, map) = deletion_fixture(false);
    app.init_asset::<bevy::world_serialization::WorldAsset>();
    crate::viewport::placement_move::interaction_tests::batch_undo_resources(&mut app);
    click_delete(&mut app);
    app.world_mut().resource_mut::<SceneEditorState>().undo_requested = true;
    app.world_mut().run_system_once(crate::authoring::apply_undo_redo).unwrap();
    let (restored, structure_revision) = {
        let session = app.world().resource::<EditorAuthoringSession>();
        assert_eq!(session.draft.as_ref().unwrap().placements.placements, vec![record.clone()]);
        assert_eq!((session.undo.len(), session.redo.len()), (0, 1));
        (session.placement_previews[&record.id], session.placement_structure_revision)
    };
    assert_eq!(app.world().get::<Visibility>(restored), Some(&Visibility::Inherited),
        "撤销删除后的新作者预览应正常显示");

    // 重新选择恢复后的实际数组槽，第二次仍经过真实按钮及新结构修订门。
    app.world_mut().resource_mut::<EditorGltfBrowserState>()
        .select_indexed_test_placement(&record.id, 0, structure_revision);
    click_delete(&mut app);
    let session = app.world().resource::<EditorAuthoringSession>();
    assert!(session.draft.as_ref().unwrap().placements.placements.is_empty());
    assert_eq!((session.undo.len(), session.redo.len()), (1, 0));
    assert!(app.world().get_entity(restored).is_err());
    let (late, parent, transform) = late_formal(&mut app, &map, &record);
    assert_hidden_without_rewriting_formal(&app, late, parent, transform);
}

#[test]
fn delete_after_the_old_limit_hides_the_target_and_keeps_prior_tickets() {
    let (mut app, preview, record, map) = deletion_fixture(false);
    let fill_map = map.clone();
    app.world_mut().run_system_once(move |mut commands: Commands,
        mut session: ResMut<EditorAuthoringSession>| {
        // 覆盖连续删除的票据上限，使用轻量元数据而非大量渲染实例。
        for index in 0..8_192 {
            crate::authoring::placement_deletion::mark_deleted(
                &mut commands, &mut session, &fill_map, &format!("capacity-deleted-{index}"),
            );
        }
    }).unwrap();
    let saved = {
        let session = app.world().resource::<EditorAuthoringSession>();
        session.placement_deletions.snapshots()[0].clone()
    };
    click_delete(&mut app);
    let session = app.world().resource::<EditorAuthoringSession>();
    assert!(session.draft.as_ref().unwrap().placements.placements.is_empty());
    assert_eq!(session.undo.len(), 1);
    assert_eq!(session.placement_deletions.snapshots().len(), 8_193);
    assert!(app.world().get_entity(preview).is_err());
    let (late, parent, transform) = late_formal(&mut app, &map, &record);
    assert_hidden_without_rewriting_formal(&app, late, parent, transform);

    // 先前保存冻结的票据独立认领，新删除仍待正式采用。
    crate::authoring::placement_deletion::acknowledge_saved_deletion(app.world_mut(), &map, &saved);
    let remaining = app.world().resource::<EditorAuthoringSession>().placement_deletions.snapshots();
    assert_eq!(remaining.len(), 8_192);
    assert!(remaining.iter().any(|snapshot| snapshot.id == record.id));
}
