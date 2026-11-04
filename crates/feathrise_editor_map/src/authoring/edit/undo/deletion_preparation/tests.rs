//! 整组历史的有限ID捕获及真实后台所有权回归，使用正式历史与删除表接口。

use super::*;
use bevy::ecs::system::RunSystemOnce;
use std::{sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}}, task::{Poll, Waker}, time::{Duration, Instant}};
use crate::authoring::{CourtyardBatchChange, placement_deletion::{acknowledge_saved_deletion, mark_deleted}};
use crate::viewport::placement_move::interaction_tests::{app as base_app, batch_undo_resources, record};

fn history_app(count: usize) -> App {
    let (mut app, _, _) = base_app();
    batch_undo_resources(&mut app);
    let records: Vec<_> = (0..count).map(|index| {
        let mut member = record();
        member.id = format!("history-preparation-{index:03}");
        member.translation = [1.0 + index as f32 / 512.0, 3.0, 2.0];
        member.water_surfaces.clear();
        member
    }).collect();
    let command = CourtyardBatchChange::new(records.clone(), Vec::new()).unwrap();
    let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
    let draft = session.draft.as_mut().unwrap();
    draft.placements.placements = records;
    draft.placement_water_surface_count = 0;
    session.undo = vec![EditorCommand::CourtyardBatch(Box::new(command))];
    session.redo.clear();
    drop(session);
    app.world_mut().resource_mut::<MapEditorState>().undo_requested = true;
    app
}

fn wait_until(mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !ready() {
        assert!(Instant::now() < deadline, "历史删除准备/退役应在有限时间内完成");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn advance(app: &mut App, state: &mut HistoryDeletionPreparation) -> Preparation {
    app.world_mut().resource_scope(|world, mut editor: Mut<MapEditorState>| {
        let session = world.resource::<EditorAuthoringSession>();
        state.maintain(session, &mut editor);
        let undoing = editor.undo_requested;
        if !undoing && !editor.redo_requested { return Preparation::Unneeded; }
        let command = if undoing { session.undo.last() } else { session.redo.last() }.unwrap();
        state.prepare(session, &editor, undoing, command)
    })
}

#[derive(Clone, Default)]
struct WorkerGate(Arc<(AtomicBool, Mutex<Option<Waker>>)>);
impl WorkerGate {
    async fn wait(&self) {
        std::future::poll_fn(|context| {
            let mut waker = self.0.1.lock().unwrap();
            if self.0.0.load(Ordering::Acquire) { Poll::Ready(()) }
            else { *waker = Some(context.waker().clone()); Poll::Pending }
        }).await;
    }
    fn release(&self) {
        let mut waker = self.0.1.lock().unwrap();
        self.0.0.store(true, Ordering::Release);
        if let Some(waker) = waker.take() { waker.wake(); }
    }
}

#[test]
fn history_captures_only_128_ids_without_popping_or_sharing_the_table_early() {
    let mut app = history_app(256);
    let mut state = HistoryDeletionPreparation::default();
    assert!(matches!(advance(&mut app, &mut state), Preparation::Waiting));
    assert_eq!(state.capture.as_ref().unwrap().cursor, 128);
    let session = app.world().resource::<EditorAuthoringSession>();
    let EditorCommand::CourtyardBatch(change) = session.undo.last().unwrap() else { panic!(); };
    let original_box = change.as_ref() as *const _ as usize;
    assert_eq!(session.undo.len(), 1);
    assert!(session.redo.is_empty());
    assert!(!session.placement_deletions.write_shared(), "未收齐ID不能提前占用作者写门");
    assert!(matches!(advance(&mut app, &mut state), Preparation::Waiting));
    assert!(state.capture.is_none());
    assert!(state.job.is_some());
    let mut prepared = None;
    wait_until(|| {
        if let Preparation::Ready(value) = advance(&mut app, &mut state) { prepared = Some(value); }
        prepared.is_some()
    });
    let session = app.world().resource::<EditorAuthoringSession>();
    let EditorCommand::CourtyardBatch(change) = session.undo.last().unwrap() else { panic!(); };
    assert_eq!(change.as_ref() as *const _ as usize, original_box, "后台准备不克隆或转移整笔Box历史");
    assert!(!session.placement_deletions.write_shared(), "worker发布完成值前必须释放源强引用");
    assert!(prepared.as_ref().unwrap().matches(&session.placement_deletions));
    assert!(session.placement_deletions.is_empty(), "准备完整结果不发布任何半成品");
}

#[test]
fn cancelled_history_keeps_the_real_worker_slot_until_it_finishes() {
    let mut app = history_app(1);
    let session = app.world().resource::<EditorAuthoringSession>();
    let identity = HistoryIdentity::new(session, true, session.undo.last().unwrap()).unwrap();
    let source = session.placement_deletions.source();
    let gate = WorkerGate::default();
    let worker_gate = gate.clone();
    let mut state = HistoryDeletionPreparation {
        job: Some(PreparationJob { identity: Some(identity), cancelled: false,
            task: AsyncComputeTaskPool::get().spawn(async move {
                worker_gate.wait().await;
                JobOutput::Prepared(source.prepare(std::iter::once("history-preparation-000"), std::iter::empty()))
            }) }),
        ..default()
    };
    app.world_mut().resource_mut::<MapEditorState>().undo_requested = false;
    assert!(matches!(advance(&mut app, &mut state), Preparation::Unneeded));
    assert!(state.job.as_ref().unwrap().cancelled);
    assert!(app.world().resource::<EditorAuthoringSession>().placement_deletions.write_shared());
    app.world_mut().resource_mut::<MapEditorState>().undo_requested = true; // 第二次请求不能超车启动另一CPU任务。
    assert!(matches!(advance(&mut app, &mut state), Preparation::Waiting));
    assert!(state.capture.is_none());
    assert!(state.job.as_ref().unwrap().cancelled);
    gate.release();
    wait_until(|| {
        app.world_mut().resource_scope(|world, mut editor: Mut<MapEditorState>| {
            state.maintain(world.resource::<EditorAuthoringSession>(), &mut editor);
        });
        state.job.is_none()
    });
    let session = app.world().resource::<EditorAuthoringSession>();
    assert!(!session.placement_deletions.write_shared());
    assert!(session.placement_deletions.is_empty());
    assert_eq!((session.undo.len(), session.redo.len()), (1, 0));
    assert!(state.ready.is_none(), "取消的完整大结果只能后台退役");
}

#[test]
fn saved_ack_revision_invalidates_prepared_history_without_an_author_change() {
    let mut app = history_app(129);
    app.world_mut().run_system_once(|mut commands: Commands, mut session: ResMut<EditorAuthoringSession>| {
        let map = session.draft.as_ref().unwrap().map_id.clone();
        mark_deleted(&mut commands, &mut session, &map, "saved-earlier");
    }).unwrap();
    let mut state = HistoryDeletionPreparation::default();
    assert!(matches!(advance(&mut app, &mut state), Preparation::Waiting));
    assert!(matches!(advance(&mut app, &mut state), Preparation::Waiting));
    wait_until(|| {
        app.world_mut().resource_scope(|world, mut editor: Mut<MapEditorState>| {
            state.maintain(world.resource::<EditorAuthoringSession>(), &mut editor);
        });
        state.ready.is_some()
    });
    let session = app.world().resource::<EditorAuthoringSession>();
    let author = session.revision;
    let deletion = session.placement_deletions.revision();
    let map = session.draft.as_ref().unwrap().map_id.clone();
    let saved = session.placement_deletions.snapshots().remove(0);
    assert!(acknowledge_saved_deletion(app.world_mut(), &map, &saved));
    assert!(matches!(advance(&mut app, &mut state), Preparation::Unneeded));
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.revision, author);
    assert_ne!(session.placement_deletions.revision(), deletion);
    assert!(session.placement_deletions.is_empty());
    assert!(state.ready.is_none());
    assert_eq!((session.undo.len(), session.redo.len()), (1, 0));
}

#[test]
fn shared_deletion_source_does_not_block_a_pure_transform_history() {
    let mut app = history_app(1);
    let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
    let before = session.draft.as_ref().unwrap().placements.placements[0].clone();
    let mut after = before.clone(); after.translation[0] += 1.0;
    session.draft.as_mut().unwrap().placements.placements[0] = after.clone();
    session.undo = vec![EditorCommand::CourtyardBatch(Box::new(
        CourtyardBatchChange::new(Vec::new(), vec![(0, before.clone(), after)]).unwrap(),
    ))];
    let deletion_revision = session.placement_deletions.revision();
    let source = session.placement_deletions.source();
    drop(session);
    app.world_mut().run_system_once(super::super::apply_undo_redo).unwrap();
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.draft.as_ref().unwrap().placements.placements, vec![before]);
    assert_eq!((session.undo.len(), session.redo.len()), (0, 1));
    assert_eq!(session.placement_deletions.revision(), deletion_revision);
    assert!(session.placement_deletions.write_shared(), "纯变换不能消费其他worker的只读来源");
    drop(source);
}

#[test]
fn added_history_256_members_undo_redo_prepares_both_deletion_directions() {
    let mut app = history_app(256);
    let records = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().placements.placements.clone();
    crate::authoring::drive_undo_redo_to_completion(&mut app);
    let session = app.world().resource::<EditorAuthoringSession>();
    assert!(session.draft.as_ref().unwrap().placements.placements.is_empty());
    assert_eq!(session.placement_deletions.len(), 256);
    assert_eq!((session.undo.len(), session.redo.len()), (0, 1));
    for _ in 0..16 {
        app.world_mut().run_system_once(crate::authoring::courtyards::sync_visuals).unwrap();
    }
    assert!(!app.world().resource::<crate::authoring::courtyards::CourtyardAuthoringState>().visual_sync_pending());
    app.world_mut().resource_mut::<MapEditorState>().redo_requested = true;
    crate::authoring::drive_undo_redo_to_completion(&mut app);
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.draft.as_ref().unwrap().placements.placements, records);
    assert!(session.placement_deletions.is_empty());
    assert!(!session.placement_deletions.write_shared());
    assert_eq!((session.undo.len(), session.redo.len()), (1, 0));
}

#[test]
fn single_placement_histories_wait_without_popping_when_deletion_source_is_shared() {
    for restoring in [false, true] {
        let mut app = history_app(1);
        app.init_asset::<bevy::world_serialization::WorldAsset>(); // 恢复正式记录缺根时沿真实Scene0预览补建路径，不读取外部GLB。
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let member = session.draft.as_ref().unwrap().placements.placements[0].clone();
        let expected = if restoring { Vec::new() } else { vec![member.clone()] };
        session.draft.as_mut().unwrap().placements.placements = expected.clone();
        session.undo = vec![if restoring { EditorCommand::PlacementRemoved {
            index: 0, record: member.clone(), had_author_preview: false,
        } } else { EditorCommand::PlacementAdded(member.clone()) }];
        let author = session.revision;
        let source = session.placement_deletions.source();
        drop(session);
        app.world_mut().run_system_once(super::super::apply_undo_redo).unwrap();
        let session = app.world().resource::<EditorAuthoringSession>();
        assert_eq!(session.draft.as_ref().unwrap().placements.placements, expected);
        assert_eq!(session.revision, author);
        assert_eq!((session.undo.len(), session.redo.len()), (1, 0));
        assert!(app.world().resource::<MapEditorState>().undo_requested);
        drop(source);
        crate::authoring::drive_undo_redo_to_completion(&mut app);
        let session = app.world().resource::<EditorAuthoringSession>();
        assert_eq!((session.undo.len(), session.redo.len()), (0, 1));
        assert_eq!(session.draft.as_ref().unwrap().placements.placements,
            if restoring { vec![member] } else { Vec::new() });
        assert_eq!(session.placement_deletions.len(), usize::from(!restoring));
    }
}
