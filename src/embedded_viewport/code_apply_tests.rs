//! 代码应用状态回归只驱动生产协调和可靠回执；有界通道替代OS游戏，不运行Cargo或用户工程。

use super::*;
use crate::embedded_viewport::{code_apply, resources::{CodeApply, CodeApplyStage}};
use crate::native_play::{CodeApplyState, CodeApplyStatus};
use feathrise_editor_protocol::Envelope;
use feathrise_editor_surface_model::SurfacePresetEditorState;

struct Fixture {
    world: World,
    runtime: Runtime,
    status: EmbeddedStatus,
    commands: mpsc::Receiver<Outgoing>,
    events: mpsc::SyncSender<Event>,
    release: mpsc::SyncSender<()>,
    camera: Entity,
}

fn project() -> crate::project::Project {
    let root = std::env::temp_dir().join("code-apply-state-only-absent");
    crate::project::Project {
        id: "code-apply-fixture".into(), name: "代码应用状态夹具".into(),
        manifest: root.join("Cargo.toml"), package: "no-game-is-launched".into(), root: root.clone(),
        context: crate::project::ProjectContext {
            root: root.clone(), id: "code-apply-fixture".into(), name: "代码应用状态夹具".into(),
            format: 1, asset_roots: vec![root.join("assets")], editor_data: root.join(".editor"),
            storage_key: "code-apply-fixture".into(), startup_scene: None,
        },
    }
}

fn identity() -> SessionIdentity {
    SessionIdentity { project_id: "code-apply-fixture".into(), session: 17, generation: 1 }
}

fn fixture(resume_playing: bool) -> Fixture {
    let project = project();
    let (sender, commands) = mpsc::sync_channel(4);
    let (events, receiver) = mpsc::sync_channel(4);
    let (result_sender, result) = mpsc::sync_channel(1);
    let (release, wait) = mpsc::sync_channel(1);
    let mut runtime = Runtime::default();
    runtime.identity = Some(identity());
    runtime.code_apply_status = Some(CodeApplyStatus { token: 42, state: CodeApplyState::Applying, error: None });
    runtime.code_apply = Some(CodeApply {
        prepared: Arc::new(crate::native_play::prepared_for_test(&project, default())),
        token: 42, resume_playing, stage: CodeApplyStage::Starting, started: Instant::now(),
        session: Some(17), restored_revision: None, mode_confirmed: false, progress: None,
    });
    runtime.active = Some(Active {
        control: default(), sender, receiver, result,
        thread: thread::spawn(move || { let _ = wait.recv(); let _ = result_sender.send(Ok(())); }),
    });
    let mut world = World::new();
    world.insert_resource(Settings { target: project.root.join("unused-target"), project });
    world.init_resource::<ViewportModeState>();
    world.init_resource::<Messages<SavedPlayFeedback>>();
    world.init_resource::<EmbeddedSnapshotState>();
    world.insert_resource(crate::native_authoring_validation::dirty_surface_fixture());
    let camera = world.spawn(Transform::from_xyz(7.0, 8.0, 9.0)).id();
    Fixture { world, runtime, status: EmbeddedStatus { active: true, ready: true, world_id: 55, ..default() },
        commands, events, release, camera }
}

fn before(fixture: &mut Fixture) {
    code_apply::before_start(&mut fixture.world, &mut fixture.runtime, &mut fixture.status,
        ViewportMode::EmbeddedWorld, default());
}

fn acknowledge(fixture: &mut Fixture, request_id: u64, mode: Mode, world_id: u64) {
    fixture.events.send(Event::Protocol(Envelope::new(identity(), request_id, Message::ModeApplied {
        request_id, mode, revision: 0, world_id,
    }))).unwrap();
    poll(&mut fixture.world, &mut fixture.runtime, &mut fixture.status);
    code_apply::after_sync(&mut fixture.world, &mut fixture.runtime, &mut fixture.status);
}

fn finish(fixture: &mut Fixture) {
    let surface = fixture.world.resource::<SurfacePresetEditorState>();
    assert!(surface.tabs[0].draft.dirty());
    assert_eq!(surface.tabs[0].draft.descriptor.label, "未保存地表·修改");
    assert_eq!(fixture.world.get::<Transform>(fixture.camera).unwrap().translation, Vec3::new(7.0, 8.0, 9.0));
    if let Some(active) = fixture.runtime.active.take() {
        fixture.release.send(()).unwrap();
        active.thread.join().unwrap();
    }
}

#[test]
fn editing_apply_waits_for_snapshot_and_exact_mode_ack_instead_of_entering_play() {
    let mut fixture = fixture(false);
    before(&mut fixture);
    fixture.runtime.snapshot_in_flight = Some(9);
    code_apply::after_sync(&mut fixture.world, &mut fixture.runtime, &mut fixture.status);
    assert!(fixture.commands.try_recv().is_err(), "未采用的完整草稿必须挡住恢复模式");
    assert_eq!(fixture.runtime.code_apply_status.as_ref().unwrap().state, CodeApplyState::Applying);
    fixture.runtime.snapshot_in_flight = None;
    code_apply::after_sync(&mut fixture.world, &mut fixture.runtime, &mut fixture.status);
    let request_id = match fixture.commands.try_recv().unwrap().message {
        Message::SetMode { request_id, mode: Mode::Editing, required_revision: 0 } => request_id,
        other => panic!("编辑态应用不能自动预览：{other:?}"),
    };
    acknowledge(&mut fixture, request_id, Mode::Editing, 999);
    assert_eq!(fixture.runtime.code_apply_status.as_ref().unwrap().state, CodeApplyState::Applying);
    acknowledge(&mut fixture, request_id, Mode::Editing, 55);
    assert_eq!(fixture.runtime.code_apply_status.as_ref().unwrap().state, CodeApplyState::Applied);
    assert!(!fixture.status.playing && fixture.runtime.code_apply.is_none());
    finish(&mut fixture);
}

#[test]
fn playing_apply_requires_a_fresh_playing_ack_after_new_world_ready() {
    let mut fixture = fixture(true);
    before(&mut fixture);
    code_apply::after_sync(&mut fixture.world, &mut fixture.runtime, &mut fixture.status);
    let request_id = match fixture.commands.try_recv().unwrap().message {
        Message::SetMode { request_id, mode: Mode::Playing, .. } => request_id,
        other => panic!("应恢复原预览模式：{other:?}"),
    };
    assert!(!fixture.status.playing, "发送恢复请求不能冒充运行实例已经接受");
    acknowledge(&mut fixture, request_id, Mode::Editing, 55);
    assert_eq!(fixture.runtime.code_apply_status.as_ref().unwrap().state, CodeApplyState::Applying);
    acknowledge(&mut fixture, request_id, Mode::Playing, 55);
    assert!(fixture.status.playing);
    assert_eq!(fixture.runtime.code_apply_status.as_ref().unwrap().state, CodeApplyState::Applied);
    finish(&mut fixture);
}

#[test]
fn apply_returns_reliably_then_waits_for_the_old_worker_to_finish() {
    let mut fixture = fixture(true);
    fixture.status.playing = true;
    fixture.runtime.code_apply.as_mut().unwrap().stage = CodeApplyStage::Returning;
    before(&mut fixture);
    assert!(fixture.runtime.pending_edit);
    assert!(!fixture.runtime.active.as_ref().unwrap().control.is_cancelled());
    assert!(request_mode(&mut fixture.runtime, &mut fixture.status, Mode::Editing, 0));
    let request_id = fixture.runtime.waiting_mode.unwrap().0;
    fixture.commands.try_recv().unwrap();
    acknowledge(&mut fixture, request_id, Mode::Editing, 55);
    before(&mut fixture);
    assert!(fixture.runtime.active.as_ref().unwrap().control.is_cancelled());
    assert!(fixture.runtime.active.is_some(), "结束意图不能冒充线程与Job已经归零");
    assert!(code_apply::ready_to_start(&fixture.runtime));
    assert_eq!(fixture.status.starts, 0, "未结束的旧槽不能启动第二份World");
    finish(&mut fixture);
}

#[test]
fn unconfirmed_cleanup_fails_the_apply_and_keeps_the_author_state() {
    let mut fixture = fixture(false);
    fixture.runtime.cleanup_failure = Some("自有夹具收口未确认".into());
    before(&mut fixture);
    assert!(fixture.runtime.code_apply.is_none());
    let apply = fixture.runtime.code_apply_status.as_ref().unwrap();
    assert_eq!((apply.token, apply.state), (42, CodeApplyState::Failed));
    assert!(apply.error.as_deref().unwrap().contains("未确认"));
    finish(&mut fixture);
}

#[test]
fn a_new_process_failure_does_not_claim_the_code_was_applied() {
    let mut fixture = fixture(false);
    fixture.status.error = Some("自有新版本握手失败".into());
    before(&mut fixture);
    assert_eq!(fixture.runtime.code_apply_status.as_ref().unwrap().state, CodeApplyState::Failed);
    assert!(fixture.runtime.code_apply.is_none());
    finish(&mut fixture);
}

#[test]
fn a_new_save_cancels_only_the_not_yet_started_candidate() {
    let mut fixture = fixture(true);
    fixture.runtime.code_apply.as_mut().unwrap().stage = CodeApplyStage::Returning;
    fixture.runtime.pending_edit = true;
    fixture.world.insert_non_send(std::mem::take(&mut fixture.runtime));
    assert!(!crate::embedded_viewport::cancel_pending_code_apply(&mut fixture.world, 999, "旧点击"));
    assert!(crate::embedded_viewport::cancel_pending_code_apply(&mut fixture.world, 42, "已有更新源码"));
    fixture.runtime = fixture.world.remove_non_send::<Runtime>().unwrap();
    assert!(fixture.runtime.code_apply.is_none() && !fixture.runtime.pending_edit && !fixture.runtime.pending_play);
    assert_eq!(fixture.runtime.code_apply_status.as_ref().unwrap().state, CodeApplyState::Failed);
    assert!(!fixture.runtime.active.as_ref().unwrap().control.is_cancelled());
    finish(&mut fixture);
}

#[test]
fn a_new_save_keeps_the_already_started_version_and_its_exact_token() {
    let mut fixture = fixture(false);
    fixture.world.insert_non_send(std::mem::take(&mut fixture.runtime));
    assert!(!crate::embedded_viewport::cancel_pending_code_apply(&mut fixture.world, 42, "已有更新源码"));
    fixture.runtime = fixture.world.remove_non_send::<Runtime>().unwrap();
    assert!(fixture.runtime.code_apply.is_some());
    assert_eq!(fixture.runtime.code_apply_status.as_ref().unwrap().state, CodeApplyState::Applying);
    assert!(!fixture.runtime.active.as_ref().unwrap().control.is_cancelled());
    finish(&mut fixture);
}
