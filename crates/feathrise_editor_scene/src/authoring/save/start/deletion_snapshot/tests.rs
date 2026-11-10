//! 删除保存的真实后台任务、请求保留、取消背压与独立删除修订回归。

use std::{
    sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}},
    task::{Poll, Waker},
    time::{Duration, Instant},
};

use bevy::{ecs::system::RunSystemOnce, prelude::*};

use super::*;
use crate::authoring::placement_deletion::{acknowledge_saved_deletion, mark_deleted, restore_deleted};

fn capture_app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(EditorAuthoringSession {
            draft: Some(crate::authoring::test_heightfield_draft(vec![0; 121].into())),
            revision: 7,
            ..default()
        });
    app
}

fn delete_ids(app: &mut App, ids: &'static [&'static str]) {
    app.world_mut().run_system_once(move |mut commands: Commands, mut session: ResMut<EditorAuthoringSession>| {
        let map = session.draft.as_ref().unwrap().scene_id.clone();
        for id in ids { mark_deleted(&mut commands, &mut session, &map, id); }
    }).unwrap();
}

fn wait_until(mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !ready() {
        assert!(Instant::now() < deadline, "真实后台删除捕获/退役应在有限时间内完成");
        std::thread::sleep(Duration::from_millis(1)); // 仅测试等待CPU任务，生产始终非阻塞轮询。
    }
}

fn wait_for_snapshot(capture: &mut DeletionSaveCapture, app: &App) {
    wait_until(|| {
        let session = app.world().resource::<EditorAuthoringSession>();
        capture.poll(session, true, None);
        capture.step(session)
    });
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

fn held_capture(app: &App) -> (DeletionSaveCapture, WorkerGate) {
    let session = app.world().resource::<EditorAuthoringSession>();
    let source = session.placement_deletions.source();
    let gate = WorkerGate::default();
    let worker_gate = gate.clone();
    let capture = DeletionSaveCapture {
        task: Some(CaptureTask::Capture {
            scene_id: session.draft.as_ref().unwrap().scene_id.clone(),
            cancelled: false,
            task: AsyncComputeTaskPool::get().spawn(async move {
                worker_gate.wait().await;
                source.snapshot()
            }),
        }),
        prepared: None,
    };
    (capture, gate)
}

#[test]
fn empty_deletions_need_no_background_capture() {
    let app = capture_app();
    let session = app.world().resource::<EditorAuthoringSession>();
    let mut capture = DeletionSaveCapture::default();
    assert!(capture.step(session));
    assert!(capture.take(session).unwrap().is_empty());
    assert!(capture.task.is_none());
}

#[test]
fn completed_snapshot_rejects_ack_revision_even_without_author_revision_change() {
    let mut app = capture_app();
    delete_ids(&mut app, &["acknowledged", "still-pending"]);
    let mut capture = DeletionSaveCapture::default();
    assert!(!capture.step(app.world().resource::<EditorAuthoringSession>()));
    wait_for_snapshot(&mut capture, &app);
    let session = app.world().resource::<EditorAuthoringSession>();
    assert!(!session.placement_deletions.write_shared(), "完成值不能继续强持源表并锁住作者写门");
    let author_revision = session.revision;
    let deletion_revision = session.placement_deletions.revision();
    let map = session.draft.as_ref().unwrap().scene_id.clone();
    let acknowledged = session.placement_deletions.snapshots().into_iter()
        .find(|saved| saved.id == "acknowledged").unwrap();
    assert!(acknowledge_saved_deletion(app.world_mut(), &map, &acknowledged));
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.revision, author_revision);
    assert_ne!(session.placement_deletions.revision(), deletion_revision);
    capture.poll(session, true, None);
    assert!(capture.prepared.is_none(), "保存确认独立换代，不能借未变化的作者修订采用旧完整删除表");
    assert!(matches!(capture.task, Some(CaptureTask::Retire(_))), "缺省共享回收资源时仍占唯一退役槽");
    assert!(capture.take(session).is_none());
    wait_for_snapshot(&mut capture, &app);
    let saved = capture.take(app.world().resource::<EditorAuthoringSession>()).unwrap();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].id, "still-pending");
}

#[test]
fn cancelled_capture_keeps_its_slot_until_terminal_and_cannot_be_revived() {
    let mut app = capture_app();
    delete_ids(&mut app, &["cancelled"]);
    let (mut capture, gate) = held_capture(&app);
    let session = app.world().resource::<EditorAuthoringSession>();
    for requested in [false, true, true] {
        capture.poll(session, requested, None);
        assert!(!capture.step(session), "旧worker未完成前不能因取消或新请求启动第二个捕获");
        assert!(matches!(capture.task, Some(CaptureTask::Capture { cancelled: true, .. })));
        assert!(session.placement_deletions.write_shared());
    }
    gate.release();
    wait_until(|| {
        capture.poll(app.world().resource::<EditorAuthoringSession>(), true, None);
        assert!(capture.prepared.is_none(), "同图新请求不能复活已取消批次");
        capture.task.is_none()
    });
    assert!(!app.world().resource::<EditorAuthoringSession>().placement_deletions.write_shared());
    assert!(!capture.step(app.world().resource::<EditorAuthoringSession>()));
    wait_for_snapshot(&mut capture, &app);
    assert_eq!(capture.take(app.world().resource::<EditorAuthoringSession>()).unwrap().len(), 1);
}

#[test]
fn missing_draft_still_polls_and_retires_the_old_capture() {
    let mut app = capture_app();
    delete_ids(&mut app, &["old-map"]);
    let (mut capture, gate) = held_capture(&app);
    app.world_mut().resource_mut::<EditorAuthoringSession>().draft = None;
    capture.poll(app.world().resource::<EditorAuthoringSession>(), true, None);
    assert!(matches!(capture.task, Some(CaptureTask::Capture { cancelled: true, .. })));
    gate.release();
    wait_until(|| {
        capture.poll(app.world().resource::<EditorAuthoringSession>(), false, None);
        capture.task.is_none()
    });
    assert!(capture.prepared.is_none());
    assert!(!app.world().resource::<EditorAuthoringSession>().placement_deletions.write_shared());
}

#[test]
fn same_scene_document_replacement_rejects_the_old_scope_before_new_capture() {
    let mut app = capture_app();
    delete_ids(&mut app, &["old-document"]);
    let (mut capture, gate) = held_capture(&app);
    // 冷载同图可保持SceneId和作者修订；新文档scope才是删除表不可混用的边界。
    let previous = std::mem::take(&mut app.world_mut().resource_mut::<EditorAuthoringSession>().placement_deletions);
    drop(previous); // 旧表仍由受控worker持有，此处只释放文档表头。
    delete_ids(&mut app, &["new-document"]);
    gate.release();
    wait_until(|| {
        capture.poll(app.world().resource::<EditorAuthoringSession>(), true, None);
        assert!(capture.prepared.is_none(), "同SceneId同作者修订也不能采用旧文档scope");
        capture.task.is_none()
    });
    wait_for_snapshot(&mut capture, &app);
    let saved = capture.take(app.world().resource::<EditorAuthoringSession>()).unwrap();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].id, "new-document");
}

#[test]
fn cancelled_completed_snapshot_uses_shared_retirement_without_a_second_local_task() {
    let mut app = capture_app();
    delete_ids(&mut app, &["cancel-after-ready"]);
    let mut capture = DeletionSaveCapture::default();
    wait_for_snapshot(&mut capture, &app);
    let mut retirement = AuthoringRetirement::default();
    capture.poll(app.world().resource::<EditorAuthoringSession>(), false, Some(&mut retirement));
    assert!(capture.prepared.is_none(), "取消后的完整候选只能移交后台退役");
    assert!(capture.task.is_none(), "共享退役资源存在时不再创建独立回收worker");
    assert!(!app.world().resource::<EditorAuthoringSession>().placement_deletions.write_shared());
    wait_for_snapshot(&mut capture, &app);
    assert_eq!(capture.take(app.world().resource::<EditorAuthoringSession>()).unwrap().len(), 1);
}

fn save_app() -> App {
    use super::super::*;

    let mut app = App::new();
    let mut draft = crate::authoring::test_heightfield_draft(vec![0; 121].into());
    // 运行真实保存启动与冻结路径；后台事务会在首写前拒绝此路径，绝不覆盖项目场景。
    draft.folder = "../deletion-save-capture-regression".into();
    app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
        .init_asset::<bevy::world_serialization::WorldAsset>()
        .insert_resource(crate::resources::SceneEditorState {
            open: true, save_requested: true, dirty: true,
            selected_scene_id: Some(draft.scene_id.clone()), ..default()
        })
        .insert_resource(EditorAuthoringSession { draft: Some(draft), revision: 7, ..default() })
        .init_resource::<DraftSaveTask>()
        .init_resource::<GlobalEnvironmentAuthoringState>()
        .init_resource::<GlobalEnvironmentSaveTask>()
        .init_resource::<VegetationCompileState>()
        .init_resource::<crate::resources::SceneDeletionTask>()
        .init_resource::<feathrise_scene_content::ActiveDynamicScene>()
        .init_resource::<feathrise_scene::DynamicWaterStatus>()
        .init_resource::<feathrise_scene::ScenePlacementRuntimeStatus>()
        .init_resource::<WaterPreviewState>()
        .init_resource::<TerrainPreviewState>()
        .init_resource::<TerrainLiveApplyState>()
        .init_resource::<PlacementTerrainFollowState>()
        .init_resource::<crate::PlacementInstanceIndex>()
        .init_resource::<crate::authoring::TerrainBrushStroke>()
        .init_resource::<WaterFileHandoff>()
        .init_resource::<WaterBrushStroke>()
        .init_resource::<PlacementWaterEvidenceRegistry>()
        .add_systems(Update, (crate::authoring::reconcile_following_placements, start_draft_save).chain());
    app
}

#[test]
fn save_request_waits_for_background_snapshot_and_freezes_exact_deletion_tickets() {
    use super::super::DraftSaveTask;

    let mut app = save_app();
    delete_ids(&mut app, &["first", "second"]);
    app.update();
    assert!(app.world().resource::<crate::resources::SceneEditorState>().save_requested,
        "后台捕获启动帧不能提前消费保存意图");
    assert!(app.world().resource::<DraftSaveTask>().task.is_none());
    wait_until(|| {
        app.update();
        app.world().resource::<DraftSaveTask>().task.is_some()
    });
    assert!(!app.world().resource::<crate::resources::SceneEditorState>().save_requested);
    assert!(app.world().resource::<EditorAuthoringSession>().saving);
    let saved = app.world().resource::<DraftSaveTask>().placement_handoff_snapshot.as_ref().unwrap().deleted.clone();
    assert_eq!(saved.iter().map(|saved| saved.id.as_str()).collect::<Vec<_>>(), ["first", "second"]);
    let map = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().scene_id.clone();
    assert!(acknowledge_saved_deletion(app.world_mut(), &map, &saved[0]));
    assert_eq!(app.world().resource::<EditorAuthoringSession>().placement_deletions.len(), 1,
        "IO保存快照须持有捕获时的真实删除票据");
    app.world_mut().run_system_once(|mut commands: Commands, mut session: ResMut<EditorAuthoringSession>| {
        let map = session.draft.as_ref().unwrap().scene_id.clone();
        restore_deleted(&mut commands, &mut session, &map, "second");
        mark_deleted(&mut commands, &mut session, &map, "second");
    }).unwrap();
    assert!(acknowledge_saved_deletion(app.world_mut(), &map, &saved[1]));
    assert_eq!(app.world().resource::<EditorAuthoringSession>().placement_deletions.len(), 1,
        "保存只冻结原弱票，不能改成完成时当前同ID删除的拥有权");
}
