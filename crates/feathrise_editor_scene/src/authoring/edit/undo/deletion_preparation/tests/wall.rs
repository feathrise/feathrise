//! 通过正式撤销入口验证无成员上限的围墙，保留非墙摆放与整笔世界同步预算。

use super::*;
use crate::authoring::{courtyards::{CourtyardAuthoringState, sync_visuals}, drive_undo_redo_to_completion};
use feathrise_scene_content::ScenePlacementRecord;

fn wall_app(base: usize, count: usize) -> (App, Vec<ScenePlacementRecord>) {
    let mut app = history_app(1);
    let records: Vec<_> = (0..base + count).map(|index| {
        let mut item = record();
        item.id = format!("wall-history-{index}");
        if index != 0 { item.water_surfaces.clear(); }
        item
    }).collect();
    let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
    let change = CourtyardBatchChange::for_wall(records[base..].to_vec(), base).unwrap();
    let draft = session.draft.as_mut().unwrap();
    draft.placements.placements = records.clone();
    draft.placement_water_surface_count = 1;
    session.undo = vec![EditorCommand::CourtyardBatch(change.into())];
    drop(session);
    (app, records)
}

#[test]
fn large_wall_undo_redo_preserves_one_command_prefix_order_and_visual_budget() {
    let (base, count) = (257, 1025);
    let (mut app, records) = wall_app(base, count);
    let mut capture = HistoryDeletionPreparation::default();
    assert!(matches!(advance(&mut app, &mut capture), Preparation::Waiting));
    assert_eq!(capture.capture.as_ref().unwrap().records.len(), CAPTURE_BUDGET);
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.draft.as_ref().unwrap().placements.placements, records);
    assert!(!session.placement_deletions.write_shared());
    let structure = session.placement_structure_revision;
    drop(capture);

    drive_undo_redo_to_completion(&mut app);
    let session = app.world().resource::<EditorAuthoringSession>();
    let draft = session.draft.as_ref().unwrap();
    assert_eq!(draft.placements.placements, records[..base]);
    assert_eq!(draft.placement_water_surface_count, 1);
    assert_eq!(session.placement_structure_revision, structure.wrapping_add(1));
    assert_eq!((session.undo.len(), session.redo.len(), session.placement_deletions.len()), (0, 1, count));
    for _ in 0..count.div_ceil(16) {
        assert!(app.world().resource::<CourtyardAuthoringState>().visual_sync_pending());
        app.world_mut().run_system_once(sync_visuals).unwrap();
    }
    assert!(!app.world().resource::<CourtyardAuthoringState>().visual_sync_pending());
    app.world_mut().resource_mut::<SceneEditorState>().redo_requested = true;
    drive_undo_redo_to_completion(&mut app);
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.draft.as_ref().unwrap().placements.placements, records);
    assert_eq!(session.draft.as_ref().unwrap().placement_water_surface_count, 1);
    assert_eq!(session.placement_structure_revision, structure.wrapping_add(2));
    assert_eq!((session.undo.len(), session.redo.len(), session.placement_deletions.len()), (1, 0, 0));
    assert!(app.world().resource::<CourtyardAuthoringState>().visual_sync_pending());
}

#[test]
fn changed_wall_tail_or_reused_prefix_id_rejects_the_whole_history() {
    for changed_prefix in [false, true] {
        let (mut app, mut expected) = wall_app(2, 305);
        if changed_prefix {
            expected[0].id = expected.last().unwrap().id.clone();
            expected.truncate(2); // 重做时某个原有物件已占用墙段 ID。
        } else { expected.last_mut().unwrap().yaw_degrees = 123.0; }
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        session.draft.as_mut().unwrap().placements.placements = expected.clone();
        if changed_prefix { session.redo = std::mem::take(&mut session.undo); }
        let revisions = (session.revision, session.placement_structure_revision);
        drop(session);
        let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
        editor.undo_requested = !changed_prefix;
        editor.redo_requested = changed_prefix;
        drop(editor);
        drive_undo_redo_to_completion(&mut app);
        let session = app.world().resource::<EditorAuthoringSession>();
        assert_eq!(session.draft.as_ref().unwrap().placements.placements, expected);
        assert_eq!((session.revision, session.placement_structure_revision), revisions);
        assert_eq!((session.undo.len(), session.redo.len(), session.placement_deletions.len()),
            (usize::from(!changed_prefix), usize::from(changed_prefix), 0));
        assert!(!app.world().resource::<CourtyardAuthoringState>().visual_sync_pending());
        assert!(app.world().resource::<SceneEditorState>().status.contains("历史记录已保留"));
    }
}

#[test]
fn wall_ready_result_refuses_changed_document_before_the_atomic_swap() {
    let (mut app, expected) = wall_app(2, 305);
    let mut state = HistoryDeletionPreparation::default();
    while state.job.is_none() { assert!(matches!(advance(&mut app, &mut state), Preparation::Waiting)); }
    wait_until(|| {
        app.world_mut().resource_scope(|world, mut editor: Mut<SceneEditorState>| {
            state.maintain(world.resource::<EditorAuthoringSession>(), &mut editor);
        });
        state.ready.is_some()
    });
    // 模拟一笔独立票据/容器换代之外的写入，仍须核对分帧冻结的摆放来源。
    app.world_mut().resource_mut::<EditorAuthoringSession>().draft.as_mut().unwrap().placement_water_surface_count += 1;
    assert!(matches!(advance(&mut app, &mut state), Preparation::Unneeded));
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.draft.as_ref().unwrap().placements.placements, expected);
    assert_eq!((session.undo.len(), session.redo.len(), session.placement_deletions.len()), (1, 0, 0));
    assert!(state.job.is_some(), "拒收的整表和视觉队列在原后台槽退役");
}
