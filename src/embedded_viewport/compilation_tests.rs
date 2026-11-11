//! 编译停止回归驱动真实协调系统；有界通道模拟迟到回执和未退出线程，不启动Cargo或实窗。

use feathrise_editor_ui::InteractionButton;
use super::*;
use crate::embedded_viewport::{can_stop_compilation, connection_state, stop_compilation, ConnectionState};
use feathrise_editor_protocol::Envelope;
use feathrise_editor_core::{ProjectBuildProfile, ProjectLaunchSettings};
use feathrise_editor_surface_model::SurfacePresetEditorState;

struct Fixture {
    world: World,
    schedule: Schedule,
    events: mpsc::SyncSender<Event>,
    commands: mpsc::Receiver<Outgoing>,
    release: mpsc::SyncSender<()>,
    control: crate::build_process::BuildControl,
    camera: Entity,
}

fn identity() -> SessionIdentity {
    SessionIdentity { project_id: "compilation-stop-fixture".into(), session: 71, generation: 1 }
}

fn fixture(outcome: Result<(), String>) -> Fixture {
    let (sender, commands) = mpsc::sync_channel(4);
    let (events, receiver) = mpsc::sync_channel(16);
    let (finished, result) = mpsc::sync_channel(1);
    let (release, wait) = mpsc::sync_channel(1);
    let (result_queued, queued) = mpsc::sync_channel(1);
    let control = crate::build_process::BuildControl::default();
    let mut runtime = Runtime::default();
    runtime.identity = Some(identity());
    runtime.attempted_selection = 1;
    runtime.attempted_profile = Some(ProjectBuildProfile::Debug);
    runtime.pending_play = true;
    runtime.restart_requested = true;
    runtime.active = Some(Active {
        control: control.clone(), sender, receiver, result,
        thread: thread::spawn(move || {
            // 先有结果、后有真实线程退出，证明只收到回执不能提前归还唯一工作槽。
            finished.send(outcome).unwrap();
            let _ = result_queued.send(());
            let _ = wait.recv();
        }),
    });
    queued.recv_timeout(Duration::from_secs(2)).unwrap();
    let mut world = World::new();
    world.init_resource::<ViewportModeState>();
    world.resource_mut::<ViewportModeState>().selected = ViewportMode::EmbeddedWorld;
    world.init_resource::<ViewportGeometry>();
    world.resource_mut::<ViewportGeometry>().parent_hwnd = 1;
    world.init_resource::<ViewportModeAvailability>();
    world.init_resource::<Messages<RunSavedProject>>();
    world.init_resource::<Messages<SavedPlayFeedback>>();
    world.init_resource::<EmbeddedAuthorInput>();
    world.init_resource::<EmbeddedSnapshotState>();
    world.resource_mut::<EmbeddedSnapshotState>().revision = 42;
    world.resource_mut::<EmbeddedSnapshotState>().scene_id = Some("author-map".into());
    world.init_resource::<crate::native_play::NativePlayStatus>();
    world.init_resource::<crate::native_project_close::NativeProjectCloseState>();
    world.init_resource::<feathrise_editor_scene::standalone::embedded_overlay::EmbeddedOverlay>();
    world.insert_resource(crate::native_authoring_validation::dirty_surface_fixture());
    world.insert_resource(EmbeddedStatus { active: true, starts: 1, ..default() });
    world.insert_non_send(runtime);
    let camera = world.spawn(Transform::from_xyz(7.0, 8.0, 9.0)).id();
    let mut schedule = Schedule::default();
    schedule.add_systems(drive);
    Fixture { world, schedule, events, commands, release, control, camera }
}

impl Fixture {
    fn frame(&mut self) { self.schedule.run(&mut self.world); }

    fn finish_worker(&mut self) {
        self.release.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while self.world.non_send::<Runtime>().active.is_some() {
            assert!(Instant::now() < deadline, "只等待本轮测试线程收口，不能无限等待");
            self.frame();
            thread::yield_now();
        }
    }

    fn assert_author_preserved(&self) {
        let surface = self.world.resource::<SurfacePresetEditorState>();
        assert!(surface.tabs[0].draft.dirty());
        assert_eq!(surface.tabs[0].draft.descriptor.label, "未保存地表·修改");
        assert_eq!(surface.tabs[0].draft.baseline.label, "未保存地表");
        assert_eq!(self.world.get::<Transform>(self.camera).unwrap().translation, Vec3::new(7.0, 8.0, 9.0));
        assert_eq!(self.world.resource::<EmbeddedSnapshotState>().revision, 42);
        assert_eq!(self.world.resource::<EmbeddedSnapshotState>().scene_id.as_deref(), Some("author-map"));
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.release.try_send(());
        if let Some(mut runtime) = self.world.get_non_send_mut::<Runtime>()
            && let Some(active) = runtime.active.take()
        {
            active.control.cancel();
            let _ = active.thread.join();
        }
    }
}

fn request_preview(world: &mut World) {
    let profile = world.get_resource::<ProjectLaunchSettings>().map_or(default(), |launch| launch.profile);
    world.write_message(RunSavedProject { profile, scene_id: None, has_unsaved_changes: true, stop_only: false });
}

/// 输入来自原控件，排空来自真实测试线程；不把控制牌取消或预先收到结果当作已经结束。
fn with_original_inputs(fixture: &mut Fixture) -> Entity {
    let (mut schedule, mode_button) = crate::viewport_mode::compilation_input_test_schedule(&mut fixture.world);
    schedule.add_systems(drive.after(crate::viewport_mode::ViewportModeInput));
    fixture.schedule = schedule;
    mode_button
}

fn assert_waiting_for_original_worker(fixture: &Fixture) {
    let runtime = fixture.world.non_send::<Runtime>();
    assert!(fixture.control.is_cancelled());
    assert!(runtime.compilation_stopped);
    assert!(runtime.active.as_ref().is_some_and(|active|!active.thread.is_finished()),
        "收到旧结果也不能跳过尚未退出的原线程");
    assert_eq!(fixture.world.resource::<ViewportModeState>().selected, ViewportMode::EmbeddedWorld);
    assert_eq!(fixture.world.resource::<ViewportModeState>().revision, 1);
    assert!(fixture.world.resource::<Messages<crate::viewport_mode::ViewportModeChanged>>().is_empty());
    assert_eq!(fixture.world.resource::<EmbeddedStatus>().starts, 1);
    assert_eq!(fixture.world.resource::<EmbeddedStatus>().completed, 0);
    assert!(fixture.world.resource::<Messages<RunSavedProject>>().is_empty());
    fixture.assert_author_preserved();
}

fn assert_one_return_after_original_worker_exits(fixture: &mut Fixture) {
    fixture.finish_worker();
    for _ in 0..32 { fixture.frame(); }
    assert!(fixture.world.non_send::<Runtime>().active.is_none());
    assert_eq!(fixture.world.resource::<ViewportModeState>().selected, ViewportMode::SavedWorld);
    assert_eq!(fixture.world.resource::<ViewportModeState>().revision, 2);
    assert_eq!(fixture.world.resource::<Messages<crate::viewport_mode::ViewportModeChanged>>().len(), 1,
        "一次明确的停止返回只能发布一次模式采用");
    assert_eq!(fixture.world.resource::<EmbeddedStatus>().starts, 1);
    assert_eq!(fixture.world.resource::<EmbeddedStatus>().completed, 1);
    assert!(fixture.world.resource::<Messages<RunSavedProject>>().is_empty(),
        "原停止点击不能成为新的预览或独立运行意图");
    fixture.assert_author_preserved();
}

#[test]
fn original_launch_stop_button_cancels_owned_worker_without_publishing_preview() {
    use feathrise_editor_ui::EditorButtonClick;
    let mut fixture = fixture(Ok(()));
    with_original_inputs(&mut fixture);
    let button = fixture.world.spawn((
        InteractionButton, feathrise_editor_core::ProjectLaunchButton, EditorButtonClick::triggered(),
        InheritedVisibility::VISIBLE,
    )).id();
    fixture.frame();
    assert!(!fixture.world.get::<EditorButtonClick>(button).unwrap().is_clicked(),
        "真实停止路由必须在原Close分派前消费同一按钮的点击");
    assert_waiting_for_original_worker(&fixture);
    // 原样式可能使停止中的按钮重新可点；重复边沿仍只被消费，不能排队新预览。
    *fixture.world.get_mut::<EditorButtonClick>(button).unwrap() = EditorButtonClick::triggered();
    for _ in 0..16 { fixture.frame(); }
    assert!(!fixture.world.get::<EditorButtonClick>(button).unwrap().is_clicked());
    assert_waiting_for_original_worker(&fixture);
    fixture.finish_worker();
    for _ in 0..32 { fixture.frame(); }
    assert!(fixture.world.non_send::<Runtime>().active.is_none());
    assert_eq!(fixture.world.resource::<ViewportModeState>().selected, ViewportMode::EmbeddedWorld);
    assert_eq!(fixture.world.resource::<ViewportModeState>().revision, 1);
    assert!(fixture.world.resource::<Messages<crate::viewport_mode::ViewportModeChanged>>().is_empty(),
        "原停止按钮只取消本次编译，不能改写视口选择及个人偏好");
    assert_eq!(fixture.world.resource::<EmbeddedStatus>().starts, 1);
    assert_eq!(fixture.world.resource::<EmbeddedStatus>().completed, 1);
    assert!(fixture.world.resource::<Messages<RunSavedProject>>().is_empty());
    fixture.assert_author_preserved();
}

#[test]
fn original_editor_3d_button_stops_compilation_and_returns_only_after_real_drain() {
    use feathrise_editor_ui::EditorButtonClick;
    let mut fixture = fixture(Err("构建已取消，作者文档保留".into()));
    let button = with_original_inputs(&mut fixture);
    *fixture.world.get_mut::<EditorButtonClick>(button).unwrap() = EditorButtonClick::triggered();
    fixture.frame();
    fixture.world.get_mut::<EditorButtonClick>(button).unwrap().invalidate();
    assert_waiting_for_original_worker(&fixture);
    for _ in 0..16 { fixture.frame(); }
    assert_waiting_for_original_worker(&fixture);
    assert_one_return_after_original_worker_exits(&mut fixture);
}

#[test]
fn original_editor_3d_button_can_request_return_while_the_stop_button_is_draining() {
    use feathrise_editor_ui::EditorButtonClick;
    let mut fixture = fixture(Ok(()));
    let mode_button = with_original_inputs(&mut fixture);
    fixture.world.spawn((InteractionButton, feathrise_editor_core::ProjectLaunchButton,
        EditorButtonClick::triggered(), InheritedVisibility::VISIBLE));
    fixture.frame();
    assert_waiting_for_original_worker(&fixture);
    *fixture.world.get_mut::<EditorButtonClick>(mode_button).unwrap() = EditorButtonClick::triggered();
    fixture.frame();
    fixture.world.get_mut::<EditorButtonClick>(mode_button).unwrap().invalidate();
    assert_waiting_for_original_worker(&fixture);
    assert_one_return_after_original_worker_exits(&mut fixture);
}

#[test]
fn explicit_stop_cancels_only_the_owned_build_and_rejects_all_late_events() {
    let mut fixture = fixture(Ok(()));
    let unrelated = crate::build_process::BuildControl::default();
    assert!(can_stop_compilation(&fixture.world));
    assert!(stop_compilation(&mut fixture.world));
    assert!(!stop_compilation(&mut fixture.world), "重复停止不能创建第二份收口意图");
    assert!(fixture.control.is_cancelled());
    assert!(!unrelated.is_cancelled());
    assert!(fixture.world.non_send::<Runtime>().identity.is_none());
    fixture.events.send(Event::Progress("迟到的构建完成".into())).unwrap();
    fixture.events.send(Event::Protocol(Envelope::new(identity(), 1, Message::Ready {
        pid: 123, hwnd: 456, world_id: 55, build_identity: "late".into(), capabilities: Vec::new(),
    }))).unwrap();
    fixture.events.send(Event::Protocol(Envelope::new(identity(), 2, Message::RequestMode { mode: Mode::Playing }))).unwrap();
    fixture.events.send(Event::Protocol(Envelope::new(identity(), 3, Message::Failure {
        request_id: 0, code: "late".into(), message: "旧任务失败".into(),
    }))).unwrap();
    let feedback_count = fixture.world.resource::<Messages<SavedPlayFeedback>>().len();
    // 取消过程的旧预览点击不得排队为排空后的自动重试。
    request_preview(&mut fixture.world);
    for _ in 0..32 { fixture.frame(); }
    let runtime = fixture.world.non_send::<Runtime>();
    assert!(runtime.active.is_some(), "实际线程尚未结束，即使结果已到达也必须占槽");
    assert!(!runtime.active.as_ref().unwrap().thread.is_finished());
    assert!(!runtime.pending_play && !runtime.pending_edit && !runtime.restart_requested);
    let status = fixture.world.resource::<EmbeddedStatus>();
    assert!(status.active && !status.ready && !status.playing);
    assert_eq!((status.pid, status.world_id, status.starts, status.completed), (0, 0, 1, 0));
    assert!(status.error.is_none());
    assert_eq!(connection_state(&fixture.world), ConnectionState::Stopping);
    assert_eq!(fixture.world.resource::<Messages<SavedPlayFeedback>>().len(), feedback_count);
    assert!(fixture.commands.try_recv().is_err());
    fixture.assert_author_preserved();
    fixture.finish_worker();
    for _ in 0..32 { fixture.frame(); }
    assert_eq!(fixture.world.resource::<EmbeddedStatus>().starts, 1);
    assert_eq!(fixture.world.resource::<EmbeddedStatus>().completed, 1);
    fixture.assert_author_preserved();
}

#[test]
fn stale_preview_click_is_discarded_when_the_cancelled_worker_drains_in_the_same_frame() {
    let mut fixture = fixture(Ok(()));
    assert!(stop_compilation(&mut fixture.world));
    fixture.release.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while !fixture.world.non_send::<Runtime>().active.as_ref().unwrap().thread.is_finished() {
        assert!(Instant::now() < deadline);
        thread::yield_now();
    }
    // 请求过滤必须先于poll：旧槽恰在此帧归还，不能把停止期间的点击解释为一次新启动。
    request_preview(&mut fixture.world);
    fixture.frame();
    assert!(fixture.world.non_send::<Runtime>().active.is_none());
    assert!(!fixture.world.non_send::<Runtime>().restart_requested);
    assert_eq!(fixture.world.resource::<EmbeddedStatus>().starts, 1);
    assert_eq!(fixture.world.resource::<EmbeddedStatus>().completed, 1);
    for _ in 0..8 { fixture.frame(); }
    assert_eq!(fixture.world.resource::<EmbeddedStatus>().starts, 1);
    fixture.assert_author_preserved();
}

#[test]
fn normal_stop_consumes_late_success_or_cancel_error_without_automatic_restart() {
    for outcome in [Ok(()), Err("构建已取消，作者文档保留".into())] {
        let mut fixture = fixture(outcome);
        // 配置已在停止的同帧变化，也不能凭旧Job与当前UI不同而再开一次构建。
        fixture.world.insert_resource(ProjectLaunchSettings {
            profile: ProjectBuildProfile::Release, preview: true, switching_profile: true,
        });
        assert!(stop_compilation(&mut fixture.world));
        fixture.finish_worker();
        for _ in 0..32 { fixture.frame(); }
        let status = fixture.world.resource::<EmbeddedStatus>();
        assert!(!status.active && !status.ready && !status.playing && status.error.is_none());
        assert_eq!((status.starts, status.completed), (1, 1));
        assert!(fixture.world.non_send::<Runtime>().compilation_stopped);
        assert_eq!(fixture.world.non_send::<Runtime>().attempted_profile, Some(ProjectBuildProfile::Debug));
        assert!(!fixture.world.resource::<ProjectLaunchSettings>().switching_profile);
        assert!(fixture.world.resource::<ViewportModeState>().status.contains("编译已停止"));
        assert!(fixture.world.resource::<ViewportModeAvailability>().blocked_reason.is_none());
        fixture.assert_author_preserved();
    }
}

#[test]
fn cleanup_failure_after_stop_keeps_the_failure_gate_and_blocks_explicit_retry() {
    let mut fixture = fixture(Err("取消后未确认工具退出".into()));
    assert!(stop_compilation(&mut fixture.world));
    fixture.control.mark_cleanup_failed();
    fixture.finish_worker();
    assert!(fixture.world.non_send::<Runtime>().cleanup_failure.is_some());
    assert!(fixture.world.resource::<EmbeddedStatus>().error.as_ref().unwrap().contains("禁止启动"));
    request_preview(&mut fixture.world);
    for _ in 0..32 { fixture.frame(); }
    assert_eq!(fixture.world.resource::<EmbeddedStatus>().starts, 1);
    assert_eq!(connection_state(&fixture.world), ConnectionState::Failed);
    assert!(fixture.world.resource::<ViewportModeAvailability>().blocked_reason.is_some());
    fixture.assert_author_preserved();
}

#[test]
fn stopping_is_unavailable_for_healthy_preview_code_apply_close_or_unconfirmed_cleanup() {
    assert!(!can_stop_compilation(&World::new()));
    for blocker in ["ready", "playing", "handoff", "return", "apply", "exit", "close-return", "close-ui", "cleanup", "control-cleanup", "already-cancelled"] {
        let mut fixture = fixture(Ok(()));
        match blocker {
            "ready" => fixture.world.resource_mut::<EmbeddedStatus>().ready = true,
            "playing" => fixture.world.resource_mut::<EmbeddedStatus>().playing = true,
            "handoff" => fixture.world.non_send_mut::<Runtime>().waiting_mode = Some((1, Mode::Editing)),
            "return" => fixture.world.non_send_mut::<Runtime>().pending_edit = true,
            "apply" => {
                let project = absent_project();
                fixture.world.non_send_mut::<Runtime>().code_apply = Some(CodeApply {
                    prepared: Arc::new(crate::native_play::prepared_for_test(&project, default())),
                    token: 9, resume_playing: false, stage: CodeApplyStage::Starting, started: Instant::now(),
                    session: Some(71), restored_revision: None, mode_confirmed: false, progress: None,
                });
            }
            "exit" => fixture.world.non_send_mut::<Runtime>().exit_requested = true,
            "close-return" => fixture.world.non_send_mut::<Runtime>().close_return_requested = true,
            "close-ui" => fixture.world.resource_mut::<crate::native_project_close::NativeProjectCloseState>().hold("关闭确认已有所有权".into()),
            "cleanup" => fixture.world.non_send_mut::<Runtime>().cleanup_failure = Some("退出尚未确认".into()),
            "control-cleanup" => fixture.control.mark_cleanup_failed(),
            "already-cancelled" => fixture.control.cancel(),
            _ => unreachable!(),
        }
        assert!(!can_stop_compilation(&fixture.world), "{blocker}仍须走原保护");
        assert!(!stop_compilation(&mut fixture.world), "{blocker}不能被停止编译绕过");
        assert!(!fixture.world.non_send::<Runtime>().compilation_stopped);
        assert_eq!(fixture.control.is_cancelled(), blocker == "already-cancelled");
        fixture.assert_author_preserved();
    }
}

#[test]
fn explicit_preview_or_later_selection_or_profile_can_retry_once_after_real_drain() {
    for action in ["preview", "selection", "profile"] {
        let mut fixture = fixture(Ok(()));
        assert!(stop_compilation(&mut fixture.world));
        fixture.finish_worker();
        for _ in 0..8 { fixture.frame(); }
        let project = absent_project();
        let root = project.root.clone();
        assert!(!root.exists());
        // 不存在的自有工程走生产prepare早退；测试重试门，不运行Cargo或创建缓存。
        fixture.world.insert_resource(Settings { target: root.join("target"), project });
        match action {
            "preview" => request_preview(&mut fixture.world),
            "selection" => fixture.world.resource_mut::<ViewportModeState>().revision += 1,
            "profile" => fixture.world.insert_resource(ProjectLaunchSettings {
                profile: ProjectBuildProfile::Release, preview: true, switching_profile: false,
            }),
            _ => unreachable!(),
        }
        fixture.frame();
        assert_eq!(fixture.world.resource::<EmbeddedStatus>().starts, 2, "{action}应明确重试一次");
        assert!(!fixture.world.non_send::<Runtime>().compilation_stopped);
        assert!(fixture.world.non_send::<Runtime>().compilation_stop_input.is_none());
        let deadline = Instant::now() + Duration::from_secs(3);
        while fixture.world.non_send::<Runtime>().active.is_some() {
            assert!(Instant::now() < deadline);
            fixture.frame();
            thread::yield_now();
        }
        for _ in 0..8 { fixture.frame(); }
        assert_eq!(fixture.world.resource::<EmbeddedStatus>().starts, 2, "失败仍不能无限重试");
        assert!(!root.exists());
        fixture.assert_author_preserved();
    }
}

fn absent_project() -> crate::project::Project {
    let root = std::env::temp_dir().join(format!("compilation-stop-absent-{}-{}", std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
    crate::project::Project {
        id: "compilation-stop-fixture".into(), name: "编译停止回归".into(),
        manifest: root.join("Cargo.toml"), package: "absent-fixture".into(), root: root.clone(),
        context: crate::project::ProjectContext {
            root: root.clone(), id: "compilation-stop-fixture".into(), name: "编译停止回归".into(),
            format: 1, asset_roots: vec![root.join("assets")], editor_data: root.join(".editor"),
            storage_key: "compilation-stop-fixture".into(), startup_scene: None,
        },
    }
}
