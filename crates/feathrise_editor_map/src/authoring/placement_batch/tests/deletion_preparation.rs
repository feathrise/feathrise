//! 整批删除准备沿真实输入/采用系统验证，完整表容量和取消均不能发布半笔历史。

use super::*;
use super::super::super::{systems::start_job, types::*, worker};
use crate::authoring::placement_deletion::{acknowledge_saved_deletion, apply_prepared};
use bevy::tasks::{AsyncComputeTaskPool, futures::check_ready};
use std::{sync::{Mutex, atomic::{AtomicBool, Ordering}}, task::{Poll, Waker}, time::{Duration, Instant}};

fn prepared_app(count: usize) -> (App, Entity) {
    let (mut app, node) = app(count);
    let members = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        for (index, record) in draft.placements.placements.iter_mut().enumerate() {
            record.translation = [1.0 + index as f32 / 512.0, 3.0, 2.0];
            record.water_surfaces.clear();
        }
        draft.placement_water_surface_count = 0;
        draft.placements.placements.iter().enumerate().map(|(index, record)| SelectedMember {
            id: record.id.clone(), index, position: record.translation.into(),
        }).collect()
    };
    app.world_mut().resource_scope(|world, mut selection: Mut<PlacementSelectionSet>| {
        selection.publish(members, world.resource::<EditorAuthoringSession>());
    });
    (app, node)
}

fn wait_until(mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !ready() {
        assert!(Instant::now() < deadline, "真实删除准备/退役应在有限时间内结束");
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// 夹具也在真实worker构造近满表，避免测试掩盖生产路径意外主线程整表复制。
fn seed_pending(app: &mut App, count: usize) {
    let source = app.world().resource::<EditorAuthoringSession>().placement_deletions.source();
    let mut task = AsyncComputeTaskPool::get().spawn(async move {
        let ids: Vec<_> = (0..count).map(|index| format!("pending-earlier-{index:04}")).collect();
        source.prepare(ids.iter().map(String::as_str), std::iter::empty()).unwrap()
    });
    let mut prepared = None;
    wait_until(|| { prepared = check_ready(&mut task); prepared.is_some() });
    let mut prepared = prepared;
    app.world_mut().run_system_once(move |mut commands: Commands, mut session: ResMut<EditorAuthoringSession>| {
        let map = session.draft.as_ref().unwrap().map_id.clone();
        apply_prepared(&mut commands, &mut session, &map, prepared.take().unwrap());
    }).unwrap();
}

fn flush_visuals(app: &mut App) {
    for _ in 0..32 {
        if !app.world().resource::<crate::authoring::courtyards::CourtyardAuthoringState>().visual_sync_pending() { return; }
        app.world_mut().run_system_once(crate::authoring::courtyards::sync_visuals).unwrap();
    }
    panic!("256项画面同步应在16帧预算内排空");
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

/// 控制真实worker完成前/后的暂停点，任务和结果始终由正式唯一槽持有。
fn held_job(app: &mut App, prepare_before_gate: bool) -> (WorkerGate, Arc<AtomicBool>) {
    let session = app.world().resource::<EditorAuthoringSession>();
    let selection = app.world().resource::<PlacementSelectionSet>();
    let draft = session.draft.as_ref().unwrap();
    let identity = BatchIdentity::from_session(session, None).unwrap();
    let capture = Capture {
        identity: identity.clone(), action: PlacementBatchAction::Delete,
        records: draft.placements.placements.iter().cloned().enumerate().collect(),
        members: selection.members.clone(), bounds: draft.manifest.world_bounds().unwrap(),
        source_objects: draft.manifest.terrain_objects_for_edit(), length: draft.placements.placements.len(),
        selection_revision: selection.revision,
        selected_id: app.world().resource::<crate::ui::gltf_browser::EditorGltfBrowserState>().selected_placement_id.clone(),
    };
    let source = session.placement_deletions.source();
    let gate = WorkerGate::default();
    let worker_gate = gate.clone();
    let reached = Arc::new(AtomicBool::new(false));
    let worker_reached = reached.clone();
    let mut state = app.world_mut().resource_mut::<PlacementBatchState>();
    start_job(&mut state, identity, async move {
        if prepare_before_gate {
            let output = worker::capture_with_deletions(capture, Some(source));
            worker_reached.store(true, Ordering::Release);
            worker_gate.wait().await;
            output
        } else {
            worker_reached.store(true, Ordering::Release);
            worker_gate.wait().await;
            worker::capture_with_deletions(capture, Some(source))
        }
    });
    (gate, reached)
}

#[test]
fn near_full_table_deletes_256_members_then_undo_redo_restores_one_atomic_history() {
    let (mut app, node) = prepared_app(256);
    seed_pending(&mut app, 7936);
    let session = app.world().resource::<EditorAuthoringSession>();
    let records = session.draft.as_ref().unwrap().placements.placements.clone();
    let author = session.revision;
    key(&mut app, node, KeyCode::Delete, Vec2::splat(0.5));
    until(&mut app, node, Vec2::splat(0.5), |state| !state.busy());
    let session = app.world().resource::<EditorAuthoringSession>();
    assert!(session.draft.as_ref().unwrap().placements.placements.is_empty());
    assert_eq!(session.placement_deletions.len(), 8192);
    assert!(!session.placement_deletions.write_shared());
    assert_eq!(session.revision, author + 1);
    assert_eq!((session.undo.len(), session.redo.len()), (1, 0));
    flush_visuals(&mut app);
    app.world_mut().resource_mut::<MapEditorState>().undo_requested = true;
    crate::authoring::drive_undo_redo_to_completion(&mut app);
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.draft.as_ref().unwrap().placements.placements, records);
    assert_eq!(session.placement_deletions.len(), 7936);
    assert_eq!((session.undo.len(), session.redo.len()), (0, 1));
    flush_visuals(&mut app);
    app.world_mut().resource_mut::<MapEditorState>().redo_requested = true;
    crate::authoring::drive_undo_redo_to_completion(&mut app);
    let session = app.world().resource::<EditorAuthoringSession>();
    assert!(session.draft.as_ref().unwrap().placements.placements.is_empty());
    assert_eq!(session.placement_deletions.len(), 8192);
    assert_eq!((session.undo.len(), session.redo.len()), (1, 0));
}

#[test]
fn over_capacity_delete_preserves_records_revisions_and_both_history_stacks() {
    let (mut app, node) = prepared_app(256);
    seed_pending(&mut app, 7937);
    let session = app.world().resource::<EditorAuthoringSession>();
    let records = session.draft.as_ref().unwrap().placements.placements.clone();
    let revisions = (session.revision, session.placement_structure_revision, session.placement_deletions.revision());
    key(&mut app, node, KeyCode::Delete, Vec2::splat(0.5));
    until(&mut app, node, Vec2::splat(0.5), |state| !state.busy());
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.draft.as_ref().unwrap().placements.placements, records);
    assert_eq!((session.revision, session.placement_structure_revision, session.placement_deletions.revision()), revisions);
    assert_eq!(session.placement_deletions.len(), 7937);
    assert!(session.undo.is_empty() && session.redo.is_empty());
    assert!(app.world().resource::<MapEditorState>().status.contains("8192"));
}

#[test]
fn completed_delete_rejects_saved_ack_change_without_an_author_revision_change() {
    let (mut app, node) = prepared_app(17);
    seed_pending(&mut app, 1);
    let session = app.world().resource::<EditorAuthoringSession>();
    let records = session.draft.as_ref().unwrap().placements.placements.clone();
    let map = session.draft.as_ref().unwrap().map_id.clone();
    let saved = session.placement_deletions.snapshots().remove(0);
    let author = session.revision;
    let (gate, reached) = held_job(&mut app, true);
    wait_until(|| reached.load(Ordering::Acquire));
    assert!(!app.world().resource::<EditorAuthoringSession>().placement_deletions.write_shared(),
        "worker虽尚未发布结果，原表强共享也必须已释放");
    assert!(acknowledge_saved_deletion(app.world_mut(), &map, &saved));
    gate.release();
    until(&mut app, node, Vec2::splat(0.5), |state| !state.busy());
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.revision, author);
    assert_eq!(session.draft.as_ref().unwrap().placements.placements, records);
    assert!(session.placement_deletions.is_empty());
    assert!(session.undo.is_empty() && session.redo.is_empty());
    assert!(app.world().resource::<MapEditorState>().status.contains("删除票据已换代"));
}

#[test]
fn escape_keeps_real_delete_worker_in_the_only_slot_until_it_and_its_result_retire() {
    let (mut app, node) = prepared_app(17);
    let records = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().placements.placements.clone();
    let (gate, reached) = held_job(&mut app, false);
    wait_until(|| reached.load(Ordering::Acquire));
    key(&mut app, node, KeyCode::Escape, Vec2::splat(0.5));
    let state = app.world().resource::<PlacementBatchState>();
    let ticket = state.ticket;
    assert!(state.job.is_some() && state.capture.is_none());
    assert!(app.world().resource::<EditorAuthoringSession>().placement_deletions.write_shared());
    key(&mut app, node, KeyCode::Delete, Vec2::splat(0.5));
    let state = app.world().resource::<PlacementBatchState>();
    assert_eq!(state.ticket, ticket, "取消期间的新点击不能另起CPU任务");
    assert!(state.job.is_some() && state.capture.is_none());
    gate.release();
    until(&mut app, node, Vec2::splat(0.5), |state| !state.busy());
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.draft.as_ref().unwrap().placements.placements, records);
    assert!(session.placement_deletions.is_empty());
    assert!(!session.placement_deletions.write_shared());
    assert!(session.undo.is_empty() && session.redo.is_empty());
}

#[test]
fn capture_rejects_same_map_same_revisions_after_document_scope_is_replaced() {
    let (mut app, node) = prepared_app(17);
    key(&mut app, node, KeyCode::Delete, Vec2::splat(0.5));
    step(&mut app, node, Vec2::splat(0.5));
    assert_eq!(app.world().resource::<PlacementBatchState>().capture.as_ref().unwrap().records.len(), 16);
    let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
    let records = session.draft.as_ref().unwrap().placements.placements.clone();
    let author = session.revision;
    session.placement_deletions = default(); // 新会话同图同记录同修订，只有弱文档scope不同。
    drop(session);
    until(&mut app, node, Vec2::splat(0.5), |state| !state.busy());
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.revision, author);
    assert_eq!(session.draft.as_ref().unwrap().placements.placements, records);
    assert!(session.placement_deletions.is_empty());
    assert!(session.undo.is_empty() && session.redo.is_empty());
}
