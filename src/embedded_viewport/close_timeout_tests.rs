//! 驱动真实关闭意图、协调系统与保存确认；仅用有界通道代替OS进程，不制造挂死子进程。

use super::*;
use feathrise_editor_protocol::Envelope;
use feathrise_editor_surface_model::SurfacePresetEditorState;

struct Fixture {
    world: World,
    schedule: Schedule,
    commands: mpsc::Receiver<Outgoing>,
    events: mpsc::SyncSender<Event>,
    release: mpsc::SyncSender<()>,
    control: crate::build_process::BuildControl,
}

fn fixture() -> Fixture {
    let (sender, commands) = mpsc::sync_channel(1);
    let (events, receiver) = mpsc::sync_channel(4);
    let (result_sender, result) = mpsc::sync_channel(1);
    let (release, wait) = mpsc::sync_channel(1);
    let control = crate::build_process::BuildControl::default();
    let mut runtime = Runtime::default();
    runtime.identity = Some(identity());
    runtime.active = Some(Active {
        control: control.clone(),
        sender,
        receiver,
        result,
        thread: thread::spawn(move || {
            let _ = wait.recv();
            let _ = result_sender.send(Ok(()));
        }),
    });
    let mut world = World::new();
    world.init_resource::<ViewportModeState>();
    world.resource_mut::<ViewportModeState>().selected = ViewportMode::EmbeddedWorld;
    world.init_resource::<ViewportGeometry>();
    world.init_resource::<ViewportModeAvailability>();
    world.init_resource::<Messages<RunSavedProject>>();
    world.init_resource::<Messages<SavedPlayFeedback>>();
    world.init_resource::<EmbeddedAuthorInput>();
    world.init_resource::<EmbeddedSnapshotState>();
    world.init_resource::<crate::native_play::NativePlayStatus>();
    world.init_resource::<crate::native_project_close::NativeProjectCloseState>();
    world.init_resource::<feathrise_editor_map::standalone::embedded_overlay::EmbeddedOverlay>();
    world.init_resource::<feathrise_editor_map::standalone::embedded_tools::EmbeddedAuthorTools>();
    world.insert_resource(crate::native_authoring_validation::dirty_surface_fixture());
    world.insert_resource(EmbeddedStatus {
        active: true,
        ready: true,
        playing: true,
        pid: 123,
        world_id: 55,
        ..default()
    });
    world.insert_non_send(runtime);
    let mut schedule = Schedule::default();
    schedule.add_systems(drive);
    Fixture { world, schedule, commands, events, release, control }
}

fn identity() -> SessionIdentity {
    SessionIdentity { project_id: "close-fixture".into(), session: 9, generation: 1 }
}

fn status(fixture: &Fixture, frame: u64) {
    fixture.events.send(Event::Protocol(Envelope::new(identity(), frame, Message::Status {
        world_id: 55,
        revision: 0,
        frame,
        ticks: frame,
        mode: Mode::Playing,
    }))).unwrap();
}

fn assert_draft_preserved(world: &World) {
    let surface = world.resource::<SurfacePresetEditorState>();
    assert!(surface.tabs[0].draft.dirty());
    assert_eq!(surface.tabs[0].draft.descriptor.label, "未保存地表·修改");
    assert_eq!(surface.tabs[0].draft.baseline.label, "未保存地表");
}

fn finish_worker(fixture: &mut Fixture) {
    fixture.release.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while !fixture.world.non_send::<Runtime>().active.as_ref().unwrap().thread.is_finished() {
        assert!(Instant::now() < deadline, "仅等待测试工作槽结束，不能无限阻塞");
        thread::yield_now();
    }
    fixture.schedule.run(&mut fixture.world);
    assert!(fixture.world.non_send::<Runtime>().active.is_none());
}

#[test]
fn close_intent_times_out_with_full_control_queue_despite_continuing_status() {
    let mut fixture = fixture();
    // 普通控制帧占满唯一入口；返回编辑本身从未入队，因此旧mode_started预算不会启动。
    fixture.world.non_send::<Runtime>().active.as_ref().unwrap().sender
        .try_send(Outgoing::control(Message::SetOverlay { revision: 1, lines: Vec::new() })).unwrap();
    assert!(!crate::embedded_viewport::return_to_edit_before_close(&mut fixture.world));
    let started = fixture.world.non_send::<Runtime>().close_return_started.unwrap();
    for frame in 1..=3 {
        status(&fixture, frame);
        fixture.schedule.run(&mut fixture.world);
        assert!(!crate::embedded_viewport::return_to_edit_before_close(&mut fixture.world));
        let runtime = fixture.world.non_send::<Runtime>();
        assert_eq!(runtime.close_return_started, Some(started));
        assert!(runtime.pending_edit && runtime.waiting_mode.is_none() && runtime.mode_started.is_none());
        assert!(!fixture.control.is_cancelled());
        assert_eq!(fixture.world.resource::<EmbeddedStatus>().frames, frame);
    }
    // 只移动关闭时钟，不等待5秒；继续由真实drive处理Status和超时，不调用测试专用成功入口。
    fixture.world.non_send_mut::<Runtime>().close_return_started = Some(Instant::now() - Duration::from_secs(6));
    status(&fixture, 4);
    fixture.schedule.run(&mut fixture.world);
    assert!(fixture.control.is_cancelled());
    assert!(fixture.world.resource::<EmbeddedStatus>().playing, "停止意图不能伪造进程退出或Editing回执");
    assert_eq!(fixture.world.resource::<EmbeddedStatus>().frames, 4);
    assert!(fixture.world.non_send::<Runtime>().waiting_mode.is_none());
    let feedback_count = fixture.world.resource::<Messages<SavedPlayFeedback>>().len();
    status(&fixture, 5);
    fixture.schedule.run(&mut fixture.world);
    assert_eq!(fixture.world.resource::<Messages<SavedPlayFeedback>>().len(), feedback_count, "收口提示不能逐帧刷屏");
    assert_draft_preserved(&fixture.world);
    assert!(!crate::embedded_viewport::return_to_edit_before_close(&mut fixture.world));
    finish_worker(&mut fixture);
    assert!(crate::embedded_viewport::return_to_edit_before_close(&mut fixture.world));
    assert!(!crate::native_project_close::authorize_exit(&mut fixture.world, true));
    assert!(fixture.world.resource::<crate::native_project_close::NativeProjectCloseState>().active());
    assert_draft_preserved(&fixture.world);
}

#[test]
fn healthy_editing_ack_and_cancel_clear_close_budget_without_stopping_world() {
    let mut fixture = fixture();
    assert!(!crate::embedded_viewport::return_to_edit_before_close(&mut fixture.world));
    fixture.schedule.run(&mut fixture.world);
    let request_id = match fixture.commands.try_recv().unwrap().message {
        Message::SetMode { request_id, mode: Mode::Editing, .. } => request_id,
        _ => panic!("关闭应优先发送真实返回编辑请求"),
    };
    // 同一帧先采用可靠ACK，再检查过期时钟；健康的成功返回不能被迟到超时误杀。
    fixture.world.non_send_mut::<Runtime>().close_return_started = Some(Instant::now() - Duration::from_secs(6));
    fixture.events.send(Event::Protocol(Envelope::new(identity(), 1, Message::ModeApplied {
        request_id, mode: Mode::Editing, revision: 0, world_id: 55,
    }))).unwrap();
    fixture.schedule.run(&mut fixture.world);
    assert!(crate::embedded_viewport::return_to_edit_before_close(&mut fixture.world));
    assert!(!crate::native_project_close::authorize_exit(&mut fixture.world, true));
    crate::native_project_close::cancel_workspace_close(&mut fixture.world);
    assert!(!crate::native_project_close::authorize_exit(&mut fixture.world, false));
    crate::embedded_viewport::cancel_exit(&mut fixture.world);
    fixture.schedule.run(&mut fixture.world);
    let runtime = fixture.world.non_send::<Runtime>();
    assert!(runtime.close_return_started.is_none() && !runtime.close_return_requested);
    assert!(!fixture.control.is_cancelled());
    let game = fixture.world.resource::<EmbeddedStatus>();
    assert!(game.active && game.ready && !game.playing);
    assert_eq!((game.pid, game.world_id), (123, 55));
    assert_draft_preserved(&fixture.world);
    // 测试退出由拥有者显式收口，取消关闭本身没有停止该工作槽。
    fixture.release.send(()).unwrap();
    fixture.world.non_send_mut::<Runtime>().active.take().unwrap().thread.join().unwrap();
}
