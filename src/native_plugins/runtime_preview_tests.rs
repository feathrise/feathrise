//! 发起身份、有限帧采用和关闭门回归；进程树与真正GPU画面另由真实窗口验收证明。
use super::*;

#[test]
fn local_author_prefab_cannot_acquire_engine_identity_or_queue_a_build() {
    let mut runtime = Runtime::default();
    let mut status = Status::default();
    let preview = RuntimePreview {
        revision: 1,
        kind: feathrise_map_content::author_visuals::AUTHOR_PREFAB_PREVIEW_KIND.into(),
        payload: serde_json::json!({"prefab":{"model":null},"playing":true,"speed":1.0}),
    };
    runtime.select(Some((0, 1, &preview)), &mut status);
    assert!(runtime.identity.is_none());
    assert!(runtime.scene.is_none());
    assert!(runtime.active.is_none());
    assert!(status.identity.is_none());
}
use std::time::{Duration, Instant};

fn preview() -> RuntimePreview {
    RuntimePreview {
        revision: 9,
        kind: "example.preview.v1".into(),
        payload: serde_json::json!({"fixture":true}),
    }
}
fn pending_worker() -> (Active, async_channel::Sender<Event>, mpsc::SyncSender<()>) {
    let (commands, _) = async_channel::bounded(2);
    let (events, incoming) = async_channel::bounded(2);
    let (done, result) = mpsc::sync_channel(1);
    let (release, wait) = mpsc::sync_channel(1);
    let handle = thread::spawn(move || {
        let _ = wait.recv();
        let _ = done.send(Ok(()));
    });
    (
        Active {
            control: BuildControl::default(),
            profile: ProjectBuildProfile::Debug,
            identity: None,
            revision: 0,
            ready: true,
            commands,
            events: incoming,
            result,
            handle,
        },
        events,
        release,
    )
}

#[test]
fn plugin_document_and_aba_navigation_never_reuse_global_frame_identity() {
    let mut runtime = Runtime::default();
    let mut status = Status::default();
    let preview = preview();
    runtime.select(Some((2, 3, &preview)), &mut status);
    let first = status.revision;
    runtime.select(Some((2, 4, &preview)), &mut status);
    assert!(status.revision > first);
    assert!(!accepts(&status, first, &preview.kind));
    runtime.select(Some((5, 4, &preview)), &mut status);
    assert!(status.revision > first + 1);
    runtime.select(Some((2, 3, &preview)), &mut status);
    assert!(status.revision > first + 2);
    assert!(!accepts(&status, first, &preview.kind));
    let current = status.revision;
    runtime.select(Some((2, 3, &preview)), &mut status);
    assert_eq!(status.revision, current, "稳定帧不重新发起或深拷草稿");
    runtime.select(None, &mut status);
    assert!(!accepts(&status, current, &preview.kind));
    assert_eq!(runtime.scene.as_ref().unwrap().message, "clear");
}

#[test]
fn late_frames_and_errors_cannot_create_image_or_replace_current_document() {
    let mut runtime = Runtime::default();
    let mut status = Status::default();
    let preview = preview();
    runtime.select(Some((2, 3, &preview)), &mut status);
    let old = status.revision;
    runtime.select(Some((2, 4, &preview)), &mut status);
    let (active, events, release) = pending_worker();
    runtime.active = Some(active);
    events
        .try_send(Event::Frame {
            revision: old,
            kind: preview.kind.clone(),
            pixels: vec![0; wire::MAX_PIXELS],
            sha256: "old".into(),
            nonuniform: false,
            evidence: serde_json::json!({"elapsed":99.0}),
        })
        .unwrap();
    events
        .try_send(Event::Error {
            revision: old,
            kind: preview.kind.clone(),
            message: "旧页错误".into(),
        })
        .unwrap();
    let mut images = Assets::<Image>::default();
    runtime.poll(&mut status, &mut images);
    assert_eq!(status.frames, 0);
    assert!(runtime.image.is_none() && status.error.is_none());
    events
        .try_send(Event::Frame {
            revision: status.revision,
            kind: preview.kind.clone(),
            pixels: vec![23; wire::MAX_PIXELS],
            sha256: "current".into(),
            nonuniform: true,
            evidence: serde_json::json!({"elapsed":0.25,"completed":true}),
        })
        .unwrap();
    runtime.poll(&mut status, &mut images);
    assert_eq!(status.frames, 1);
    assert_eq!(status.rendered_revision, status.revision);
    assert_eq!(
        status.runtime_evidence,
        serde_json::json!({"elapsed":0.25,"completed":true})
    );
    assert_eq!(
        images
            .get(runtime.image.as_ref().unwrap().id())
            .unwrap()
            .data
            .as_ref()
            .unwrap()
            .len(),
        wire::MAX_PIXELS
    );
    let active = runtime.active.take().unwrap();
    runtime.select(None, &mut status);
    assert!(
        status.runtime_evidence.is_null(),
        "切文档必须同时撤销旧运行证据"
    );
    release.send(()).unwrap();
    active.handle.join().unwrap();
}

#[test]
fn original_close_gate_cancels_owned_worker_and_waits_for_actual_completion() {
    let (active, _, release) = pending_worker();
    let control = active.control.clone();
    let mut world = World::new();
    let mut runtime = Runtime::default();
    runtime.active = Some(active);
    world.insert_non_send(runtime);
    assert_eq!(ready_to_exit(&mut world), Ok(false));
    assert!(control.is_cancelled());
    assert_eq!(
        ready_to_exit(&mut world),
        Ok(false),
        "取消信号不能冒充线程已经退出"
    );
    release.send(()).unwrap();
    let started = Instant::now();
    while !world
        .non_send::<Runtime>()
        .active
        .as_ref()
        .unwrap()
        .handle
        .is_finished()
    {
        assert!(started.elapsed() < Duration::from_secs(5));
        thread::yield_now();
    }
    let mut status = Status::default();
    let mut images = Assets::<Image>::default();
    world
        .non_send_mut::<Runtime>()
        .poll(&mut status, &mut images);
    assert_eq!(ready_to_exit(&mut world), Ok(true));
    world.non_send_mut::<Runtime>().cleanup_failure = Some("夹具收口未确认".into());
    assert!(ready_to_exit(&mut world).is_err());
}

#[test]
fn close_without_installed_preview_is_immediately_quiescent() {
    assert_eq!(ready_to_exit(&mut World::new()), Ok(true));
}

#[test]
fn new_document_sends_default_camera_after_preview_even_under_queue_backpressure() {
    let mut runtime = Runtime::default();
    let mut status = Status::default();
    let preview = preview();
    let (mut active, _, release) = pending_worker();
    let (sender, requests) = async_channel::bounded(2);
    active.commands = sender;
    runtime.active = Some(active);
    runtime.select(Some((1, 2, &preview)), &mut status);
    runtime.dispatch(); // 两个槽装满旧preview和旧镜头，模拟真实后台尚未取数。
    runtime.camera_angles = Vec3::new(2., 1., 20.);
    runtime.select(Some((1, 3, &preview)), &mut status);
    let revision = status.revision;
    runtime.dispatch();
    assert!(runtime.scene.is_some() && runtime.camera.is_some());
    assert_eq!(requests.try_recv().unwrap().message, "preview");
    assert_eq!(requests.try_recv().unwrap().message, "camera");
    runtime.dispatch();
    let scene = requests.try_recv().unwrap();
    let camera = requests.try_recv().unwrap();
    assert_eq!((scene.message, camera.message), ("preview", "camera"));
    assert_eq!((scene.revision, camera.revision), (revision, revision));
    assert_eq!(
        camera.data,
        serde_json::json!({"yaw":0.675_f32,"pitch":0.245_f32,"distance":6.6_f32})
    );
    let packet = wire::Packet {
        project: "camera-queue-fixture".into(),
        session: 1,
        revision: camera.revision,
        message: camera.message.into(),
        kind: camera.kind,
        data: camera.data,
    };
    let mut bytes = Vec::new();
    wire::write(&mut bytes, &packet, &[]).unwrap();
    let decoded = wire::read(&mut std::io::Cursor::new(bytes))
        .unwrap()
        .unwrap()
        .0;
    assert_eq!(decoded.revision, revision);
    runtime.select(None, &mut status);
    runtime.dispatch();
    assert_eq!(requests.try_recv().unwrap().message, "clear");
    assert!(requests.try_recv().is_err(), "没有预览时不发送默认镜头");
    let active = runtime.active.take().unwrap();
    release.send(()).unwrap();
    active.handle.join().unwrap();
}

#[test]
fn actual_camera_input_system_requires_a_new_right_press_after_document_revision_changes() {
    let mut world = World::new();
    world.insert_non_send(Runtime::default());
    world.insert_resource(Status {
        ready: true,
        ..default()
    });
    world.insert_resource(ButtonInput::<MouseButton>::default());
    world.insert_resource(AccumulatedMouseMotion {
        delta: Vec2::new(12., 4.),
    });
    world.insert_resource(bevy::ecs::message::Messages::<MouseWheel>::default());
    world.spawn((
        RuntimeCanvas,
        Node::default(),
        bevy::ui::RelativeCursorPosition {
            cursor_over: true,
            normalized: Some(Vec2::splat(0.5)),
        },
    ));
    let preview = preview();
    let mut status = world.remove_resource::<Status>().unwrap();
    world
        .non_send_mut::<Runtime>()
        .select(Some((1, 2, &preview)), &mut status);
    world.insert_resource(status);
    let mut schedule = Schedule::default();
    schedule.add_systems(camera_input);
    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Right);
    schedule.run(&mut world);
    assert_ne!(
        world.non_send::<Runtime>().camera_angles,
        Vec3::new(0.675, 0.245, 6.6)
    );
    world.resource_mut::<ButtonInput<MouseButton>>().clear(); // 继续按住，只有按下边沿被消费。
    let mut status = world.remove_resource::<Status>().unwrap();
    world
        .non_send_mut::<Runtime>()
        .select(Some((1, 3, &preview)), &mut status);
    world.insert_resource(status);
    schedule.run(&mut world);
    assert_eq!(
        world.non_send::<Runtime>().camera_angles,
        Vec3::new(0.675, 0.245, 6.6),
        "旧文档拖动不能改写新文档默认镜头"
    );
    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Right);
    schedule.run(&mut world);
    world.resource_mut::<ButtonInput<MouseButton>>().clear();
    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Right);
    schedule.run(&mut world);
    assert_ne!(
        world.non_send::<Runtime>().camera_angles,
        Vec3::new(0.675, 0.245, 6.6)
    );
}

#[test]
fn actual_error_event_is_terminal_for_late_frames_but_next_preview_can_recover() {
    let mut runtime = Runtime::default();
    let mut status = Status::default();
    let mut preview = preview();
    runtime.select(Some((1, 2, &preview)), &mut status);
    let (active, events, release) = pending_worker();
    runtime.active = Some(active);
    let mut images = Assets::<Image>::default();
    let frame = |revision, pixel| Event::Frame {
        revision,
        kind: "example.preview.v1".into(),
        pixels: vec![pixel; wire::MAX_PIXELS],
        sha256: format!("frame-{pixel}"),
        nonuniform: true,
        evidence: serde_json::json!({"elapsed":0.25}),
    };
    events.try_send(frame(status.revision, 10)).unwrap();
    runtime.poll(&mut status, &mut images);
    assert_eq!(status.frames, 1);
    events
        .try_send(Event::Error {
            revision: status.revision,
            kind: preview.kind.clone(),
            message: "当前预览终态失败".into(),
        })
        .unwrap();
    events.try_send(frame(status.revision, 90)).unwrap();
    runtime.poll(&mut status, &mut images);
    assert_eq!(status.error.as_deref(), Some("当前预览终态失败"));
    assert_eq!(status.rendered_revision, 0);
    assert_eq!(status.frames, 1);
    assert!(status.runtime_evidence.is_null());
    assert_eq!(
        images
            .get(runtime.image.as_ref().unwrap().id())
            .unwrap()
            .data
            .as_ref()
            .unwrap()[0],
        10
    );
    preview.revision += 1;
    runtime.select(Some((1, 2, &preview)), &mut status);
    events.try_send(frame(status.revision, 77)).unwrap();
    runtime.poll(&mut status, &mut images);
    assert!(status.error.is_none());
    assert_eq!(status.rendered_revision, status.revision);
    assert_eq!(status.frames, 2);
    let active = runtime.active.take().unwrap();
    release.send(()).unwrap();
    active.handle.join().unwrap();
}

fn failed_build(identity: Option<Identity>, revision: u64, control: BuildControl) -> Active {
    let (commands, _) = async_channel::bounded(2);
    let (_, events) = async_channel::bounded(2);
    let (sender, result) = mpsc::sync_channel(1);
    let handle = thread::spawn(move || { let _ = sender.send(Err("真实构建失败：anon.fixture.llvm.7".into())); });
    Active { control, profile: ProjectBuildProfile::Debug, identity, revision, ready: false,
        commands, events, result, handle }
}

fn await_worker(runtime: &Runtime) {
    let started = Instant::now();
    while !runtime.active.as_ref().unwrap().handle.is_finished() {
        assert!(started.elapsed() < Duration::from_secs(5));
        thread::yield_now();
    }
}

#[test]
fn stale_build_failure_keeps_new_document_request_until_a_new_worker_really_receives_it() {
    let mut runtime = Runtime::default();
    let mut status = Status::default();
    let mut images = Assets::<Image>::default();
    let old = preview();
    runtime.select(Some((1, 2, &old)), &mut status);
    runtime.active = Some(failed_build(runtime.identity.clone(), runtime.revision, BuildControl::default()));
    let mut current = preview();
    current.revision += 1;
    current.payload = serde_json::json!({"new_document":"未保存正文"});
    runtime.select(Some((1, 3, &current)), &mut status);
    runtime.dispatch();
    assert!(runtime.scene.is_some(), "构建中不能把新正文交给即将失败的旧队列");
    await_worker(&runtime);
    runtime.poll(&mut status, &mut images);
    assert!(status.error.is_none());
    assert!(runtime.can_start(), "明确的新文档请求没有被旧失败标成终态");

    // 由实际后台线程回传Ready，再走正式dispatch；不是直接改成功标记冒充重试。
    let (commands, incoming) = async_channel::bounded::<Request>(2);
    let (events, event_rx) = async_channel::bounded(2);
    let (sender, result) = mpsc::sync_channel(1);
    let (observed, requests) = mpsc::sync_channel(1);
    let (release, wait) = mpsc::sync_channel(1);
    let handle = thread::spawn(move || {
        let _ = events.send_blocking(Event::Ready(77));
        if let Ok(request) = incoming.recv_blocking() { let _ = observed.send(request); }
        let _ = wait.recv();
        let _ = sender.send(Ok(()));
    });
    runtime.active = Some(Active { control: BuildControl::default(), profile: ProjectBuildProfile::Debug,
        identity: runtime.identity.clone(), revision: runtime.revision, ready: false,
        commands, events: event_rx, result, handle });
    let started = Instant::now();
    while !status.ready {
        assert!(started.elapsed() < Duration::from_secs(5));
        runtime.poll(&mut status, &mut images);
        thread::yield_now();
    }
    runtime.dispatch();
    let request = requests.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(request.message, "preview");
    assert_eq!(request.revision, runtime.revision);
    assert_eq!(request.data, current.payload);
    release.send(()).unwrap();
    runtime.active.take().unwrap().handle.join().unwrap();
}

#[test]
fn current_build_failure_keeps_real_diagnostic_and_does_not_automatically_retry() {
    let channel = crate::project_log::fixture_channel("作者任务失败回归");
    let control = BuildControl::with_log(channel.begin_task(crate::project_log::LogTaskKind::AuthorPreview));
    let mut runtime = Runtime::default();
    let mut status = Status::default();
    let mut images = Assets::<Image>::default();
    let mut preview = preview();
    runtime.select(Some((1, 2, &preview)), &mut status);
    runtime.active = Some(failed_build(runtime.identity.clone(), runtime.revision, control));
    await_worker(&runtime);
    runtime.poll(&mut status, &mut images);
    let error = status.error.as_deref().unwrap();
    assert!(error.contains("作者预览 #1") && error.contains("anon.fixture.llvm.7"));
    assert!(!runtime.can_start(), "同一失败身份不能每帧重新创建构建");
    preview.revision += 1;
    runtime.select(Some((1, 2, &preview)), &mut status);
    assert!(runtime.can_start(), "新的显式预览修订允许重试");
}

#[test]
fn aba_navigation_cannot_adopt_a_late_build_failure_from_the_first_visit() {
    let mut runtime = Runtime::default();
    let mut status = Status::default();
    let mut images = Assets::<Image>::default();
    let preview = preview();
    runtime.select(Some((1, 2, &preview)), &mut status);
    runtime.active = Some(failed_build(runtime.identity.clone(), runtime.revision, BuildControl::default()));
    runtime.select(Some((1, 3, &preview)), &mut status);
    runtime.select(Some((1, 2, &preview)), &mut status);
    await_worker(&runtime);
    runtime.poll(&mut status, &mut images);
    assert!(status.error.is_none() && runtime.can_start(), "完整主线修订必须区分相同文档的两次访问");
}
