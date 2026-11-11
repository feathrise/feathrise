//! 实际数值事务与正式完整快照交接；取消后的权威值不得被同一旧修订的迟到编码覆盖。
use super::*;
use crate::{authoring::EditorAuthoringSession, resources::SceneEditorState,
    ui::placement_transform::{PlacementTransformField, handle_placement_transform_numeric_input},
    viewport::placement_move::interaction_tests as original};
use bevy::tasks::AsyncComputeTaskPool;
use feathrise_scene_content::embedded_snapshot::SceneSnapshot;
use feathrise_editor_ui::{NumericInputChanged, NumericInputCommitOutcome, NumericInputCommitSource,
    NumericInputCommitted, NumericInputScrubSensitivityChanged, NumericInputScrubSensitivityCommitted,
    NumericInputSetValue, NumericValue};
use std::{sync::Arc, time::{Duration, Instant}};

fn change(app: &mut App, input: Entity, value: f32) {
    app.world_mut().write_message(NumericInputChanged { entity: input, value: NumericValue::F32(value) });
    app.update();
}
fn finish(app: &mut App, input: Entity, value: f32, outcome: NumericInputCommitOutcome) {
    app.world_mut().write_message(NumericInputCommitted { entity: input, value: NumericValue::F32(value),
        outcome, source: NumericInputCommitSource::Scrub, changed: true, edited: true });
    app.update();
}
fn settle(app: &mut App) -> Arc<EmbeddedSnapshot> {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        app.update();
        let state = app.world().resource::<EmbeddedSnapshotState>();
        assert!(state.error.is_none(), "{:?}", state.error);
        if ready_is_current(app.world()) { return state.ready.as_ref().unwrap().clone(); }
        assert!(Instant::now() < until, "取消后的完整来源必须有限时间收口");
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn x(snapshot: &EmbeddedSnapshot) -> f32 {
    let value: SceneSnapshot = serde_json::from_slice(&snapshot.bytes).unwrap();
    value.placements.placements[0].translation[0]
}

#[test]
fn numeric_cancel_invalidates_late_full_snapshot_and_keeps_committed_authority() {
    let (mut app, root, _) = original::app();
    app.init_resource::<feathrise_editor_core::EditorSettings>()
        .add_message::<NumericInputChanged>().add_message::<NumericInputCommitted>()
        .add_message::<NumericInputScrubSensitivityChanged>().add_message::<NumericInputScrubSensitivityCommitted>()
        .add_message::<NumericInputSetValue>()
        .add_systems(Update, handle_placement_transform_numeric_input.in_set(crate::SceneEditorSystemSet::Tasks))
        .add_plugins(SnapshotCapturePlugin);
    let input = app.world_mut().spawn(PlacementTransformField::TranslationX).id();
    app.world_mut().resource_mut::<EmbeddedSnapshotState>().enabled = true;
    assert_eq!(x(&settle(&mut app)), 5.0);

    change(&mut app, input, 9.0);
    assert!(systems::busy_reason(app.world()).is_some(), "业务scrub独立于实体鼠标guard阻止完整捕获");
    assert!(!ready_is_current(app.world()));
    finish(&mut app, input, 9.0, NumericInputCommitOutcome::Accepted);
    assert_eq!(x(&settle(&mut app)), 9.0);
    assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 1);

    change(&mut app, input, 10.0);
    let old_revision = app.world().resource::<EditorAuthoringSession>().revision;
    let state = app.world().resource::<EmbeddedSnapshotState>();
    assert!(state.pending && state.capture.is_none() && state.task.is_none());
    let old_snapshot_revision = state.revision;
    // 故意冻结真实临时10的生产格式，模拟此前已经跨入后台的编码任务；仅控制完成时机。
    let source = capture::source_identity(app.world()).unwrap();
    let mut captured = capture::Capture::new(app.world(), source.clone()).unwrap();
    while !captured.step(app.world()).unwrap() {}
    assert_eq!(captured.placements.placements[0].translation[0], 10.0);
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    app.world_mut().resource_mut::<EmbeddedSnapshotState>().task = Some(AsyncComputeTaskPool::get().spawn(async move {
        receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        let result = encode::encode(captured).map(|bytes| Arc::new(EmbeddedSnapshot {
            scene_id: source.scene_id.0.clone(), author_revision: old_snapshot_revision, bytes: Arc::new(bytes), pose_targets: Default::default() }));
        (source, old_snapshot_revision, result)
    }));
    finish(&mut app, input, 10.0, NumericInputCommitOutcome::Canceled);
    let session = app.world().resource::<EditorAuthoringSession>();
    assert!(session.revision > old_revision, "回滚是新权威值，不能继续给临时10的快照信用");
    assert_eq!(session.draft.as_ref().unwrap().placements.placements[0].translation[0], 9.0);
    assert_eq!(session.undo.len(), 1, "取消不生成第二条历史");
    assert!(session.redo.is_empty());
    assert_eq!(session.successful_save_sequence, 0);
    assert!(app.world().resource::<SceneEditorState>().dirty, "保留第一笔未保存提交的dirty");
    assert_eq!(app.world().get::<Transform>(root).unwrap().translation.x, 9.0);
    assert!(systems::busy_reason(app.world()).is_none(), "取消后业务忙门必须归还");
    assert!(!ready_is_current(app.world()), "旧10的任务完成前不能发起预览");
    sender.send(()).unwrap();
    let ready = settle(&mut app);
    assert!(ready.author_revision > old_snapshot_revision);
    assert_eq!(x(&ready), 9.0);
    for _ in 0..8 { app.update(); }
    assert!(ready_is_current(app.world()));
    assert_eq!(x(app.world().resource::<EmbeddedSnapshotState>().ready.as_ref().unwrap()), 9.0);
    assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 1);
}
