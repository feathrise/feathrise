//! 整批Delete成功门发布删除责任，失败保持原值，撤销与重做不等待分帧画面才换代。

use super::*;
use feathrise_scene_content::ScenePlacementRecord;
use feathrise_scene::ScenePlacementInstance;

fn observe_roots(app: &mut App) {
    app.add_observer(crate::ui::placement_water::register_placement_instance_root)
        .add_observer(crate::ui::placement_water::unregister_placement_instance_root);
}

fn late_formal(app: &mut App, map: &SceneId, record: &ScenePlacementRecord) -> Entity {
    let root = app.world_mut().spawn((ScenePlacementInstance::new(map, record),
        Transform::from_xyz(25.0, 26.0, 27.0), Visibility::Inherited)).id();
    app.world_mut().flush();
    root
}

#[test]
fn failed_batch_delete_does_not_publish_deletion_for_any_late_formal_root() {
    let (mut app, node) = app(2);
    observe_roots(&mut app);
    let (map, before, revisions) = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let revisions = (session.revision, session.placement_structure_revision);
        let draft = session.draft.as_mut().unwrap();
        // 实际记录携带两个水绑定，但缓存不足；让完整后台快照通过后在主线程采用门拒绝。
        draft.placement_water_surface_count = 0;
        (draft.scene_id.clone(), draft.placements.placements.clone(), revisions)
    };
    key(&mut app, node, KeyCode::Delete, Vec2::ZERO);
    until(&mut app, node, Vec2::ZERO, |state| !state.busy());
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.draft.as_ref().unwrap().placements.placements, before);
    assert_eq!((session.revision, session.placement_structure_revision), revisions);
    assert!(session.undo.is_empty() && session.redo.is_empty());
    assert!(app.world().resource::<SceneEditorState>().status.contains("整批删除未应用"));
    for record in &before {
        let root = late_formal(&mut app, &map, record);
        assert_eq!(app.world().get::<Visibility>(root), Some(&Visibility::Inherited),
            "删除失败不能发布任何成员的删除责任");
    }
}

#[test]
fn batch_delete_undo_and_redo_publish_all_ids_before_the_visual_queue_runs() {
    // 第十七项超出单帧十六根视觉预算；它也必须在整批数据采用后立即受删除责任保护。
    let (mut app, node) = app(17);
    observe_roots(&mut app);
    let (map, before) = {
        let session = app.world().resource::<EditorAuthoringSession>();
        let draft = session.draft.as_ref().unwrap();
        (draft.scene_id.clone(), draft.placements.placements.clone())
    };
    key(&mut app, node, KeyCode::Delete, Vec2::ZERO);
    until(&mut app, node, Vec2::ZERO, |state| !state.busy());
    assert!(app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap()
        .placements.placements.is_empty());
    let last = before.last().unwrap();
    let late_deleted = late_formal(&mut app, &map, last);
    assert_eq!(app.world().get::<Visibility>(late_deleted), Some(&Visibility::Hidden),
        "删除责任必须早于第十七项分帧清理发布");

    for _ in 0..2 {
        app.world_mut().run_system_once(crate::authoring::courtyards::sync_visuals).unwrap();
    }
    app.world_mut().resource_mut::<SceneEditorState>().undo_requested = true;
    crate::authoring::drive_undo_redo_to_completion(&mut app);
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.draft.as_ref().unwrap().placements.placements, before);
    assert_eq!((session.undo.len(), session.redo.len()), (0, 1));
    // 恢复视觉队列尚未建立新作者预览，迟到根可见直接证明成功撤销已撤回删除票据。
    let late_restored = late_formal(&mut app, &map, last);
    assert_eq!(app.world().get::<Visibility>(late_restored), Some(&Visibility::Inherited));

    for _ in 0..2 {
        app.world_mut().run_system_once(crate::authoring::courtyards::sync_visuals).unwrap();
    }
    app.world_mut().resource_mut::<SceneEditorState>().redo_requested = true;
    crate::authoring::drive_undo_redo_to_completion(&mut app);
    let session = app.world().resource::<EditorAuthoringSession>();
    assert!(session.draft.as_ref().unwrap().placements.placements.is_empty());
    assert_eq!((session.undo.len(), session.redo.len()), (1, 0));
    let late_redone = late_formal(&mut app, &map, last);
    assert_eq!(app.world().get::<Visibility>(late_redone), Some(&Visibility::Hidden),
        "重做删除也必须早于分帧视觉清理发布新责任");
}

#[test]
fn batch_deletion_masks_only_sixteen_existing_ids_per_frame_but_every_late_id_immediately() {
    use crate::authoring::placement_deletion::PlacementDeletionSuppression;

    let (mut app, node) = app(17);
    observe_roots(&mut app);
    let (map, records) = {
        let session = app.world().resource::<EditorAuthoringSession>();
        let draft = session.draft.as_ref().unwrap();
        (draft.scene_id.clone(), draft.placements.placements.clone())
    };
    let formal_roots = records.iter().map(|record| late_formal(&mut app, &map, record)).collect::<Vec<_>>();
    key(&mut app, node, KeyCode::Delete, Vec2::ZERO);
    until(&mut app, node, Vec2::ZERO, |state| !state.busy());
    assert!(app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap()
        .placements.placements.is_empty());
    assert!(formal_roots.iter().all(|&root| app.world().get::<PlacementDeletionSuppression>(root).is_none()),
        "整批作者采用只登记逻辑表，不能同步遍历所有已有根");

    // 每一个稳定ID都立即拥有删除票据；它的新正式根不依赖旧根的视觉队列处理进度。
    for record in &records {
        let late = late_formal(&mut app, &map, record);
        assert!(app.world().get::<PlacementDeletionSuppression>(late).is_some());
        assert_eq!(app.world().get::<Visibility>(late), Some(&Visibility::Hidden));
    }
    app.world_mut().run_system_once(crate::authoring::courtyards::sync_visuals).unwrap();
    assert_eq!(formal_roots.iter().filter(|&&root|
        app.world().get::<PlacementDeletionSuppression>(root).is_some()).count(), 16,
        "已有正式根的一次视觉同步只消费十六个ID");
    for &root in &formal_roots {
        let expected = if app.world().get::<PlacementDeletionSuppression>(root).is_some() {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        assert_eq!(app.world().get::<Visibility>(root), Some(&expected));
    }
    app.world_mut().run_system_once(crate::authoring::courtyards::sync_visuals).unwrap();
    assert!(formal_roots.iter().all(|&root| app.world().get::<PlacementDeletionSuppression>(root).is_some()));
}
