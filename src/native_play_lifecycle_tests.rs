//! 工程运行收口的真实系统回归：非零/后台错误要发布失败并释放唯一槽，下一次请求仍可进入工作器。

use super::*;
use std::time::{Duration, Instant};

#[derive(Resource, Clone, Debug, PartialEq, Eq)]
struct AuthorProof {
    draft: String,
    history: Vec<String>,
}

#[derive(Component)]
struct SameAuthorWorld;

fn app() -> (App, Entity, AuthorProof) {
    // 此目录刻意不创建，后续重试只走真实worker的“工程缺失”早退，测试不启动Cargo或游戏。
    let root = std::env::temp_dir().join(format!(
        "native-play-lifecycle-absent-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    assert!(!root.exists());
    let mut app = App::new();
    app.init_non_send::<NativePlayRuntime>()
        .insert_resource(NativePlaySettings {
            project: crate::project::Project {
                id: "lifecycle-test".into(),
                name: "工程运行失败恢复".into(),
                manifest: root.join("Cargo.toml"),
                package: "absent-test-game".into(),
                root: root.clone(),
                context: crate::project::ProjectContext {
                    root: root.clone(),
                    id: "lifecycle-test".into(),
                    name: "工程运行失败恢复".into(),
                    format: 1,
                    asset_roots: vec![root.join("assets")],
                    editor_data: root.join(".editor"),
                    storage_key: "lifecycle-test".into(),
                    startup_scene: None,
                },
            },
            target: root.join("target"),
        })
        .insert_resource(NativePlayStatus {
            started: 1,
            active: true,
            ..default()
        })
        .init_resource::<StandaloneEditorMode>()
        .add_message::<RunSavedProject>()
        .add_message::<SavedPlayFeedback>()
        .add_systems(Update, (poll_play, request_play).chain());
    app.world_mut()
        .resource_mut::<StandaloneEditorMode>()
        .set_external_play_active(true);
    let proof = AuthorProof {
        draft: "尚未保存的原作者内容".into(),
        history: vec!["放置".into(), "移动".into(), "撤销".into()],
    };
    app.insert_resource(proof.clone());
    let marker = app.world_mut().spawn(SameAuthorWorld).id();
    (app, marker, proof)
}

fn install_completed_worker(app: &mut App, result: Result<u32, String>) {
    app.world_mut().resource_mut::<NativePlayStatus>().preparing = true;
    let (_, progress_rx) = mpsc::sync_channel(4);
    let (sender, result_rx) = mpsc::sync_channel(1);
    let handle = thread::spawn(move || {
        let _ = sender.send(result);
    });
    app.world_mut().non_send_mut::<NativePlayRuntime>().active = Some(resources::ActivePlay {
        control: crate::build_process::BuildControl::default(),
        progress_rx,
        result_rx,
        handle,
    });
}

fn wait_for_completion(app: &mut App, expected: u64) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.world().resource::<NativePlayStatus>().completed < expected {
        assert!(Instant::now() < deadline, "测试夹具线程未在时限内回收");
        app.update();
        thread::yield_now(); // 只让测试自有线程获得调度；生产poll本身仍严格非阻塞。
    }
}

fn assert_retry(app: &mut App, marker: Entity, proof: &AuthorProof) {
    let runtime = app.world().non_send::<NativePlayRuntime>();
    assert!(runtime.active.is_none());
    assert!(!runtime.cleanup_failed);
    assert!(!runtime.exit_requested);
    assert!(app.world().get::<SameAuthorWorld>(marker).is_some());
    assert_eq!(app.world().resource::<AuthorProof>(), proof);
    app.world_mut().write_message(RunSavedProject {
        profile: default(),
        map_id: None,
        has_unsaved_changes: true,
        stop_only: false,
    });
    app.update();
    assert_eq!(
        app.world().resource::<NativePlayStatus>().started,
        2,
        "失败后新请求必须进入真实worker"
    );
    assert!(app.world().resource::<NativePlayStatus>().preparing);
    wait_for_completion(app, 2);
    assert!(!app.world().resource::<NativePlayStatus>().preparing);
    assert!(app.world().non_send::<NativePlayRuntime>().active.is_none());
    assert!(app.world().get::<SameAuthorWorld>(marker).is_some());
    assert_eq!(app.world().resource::<AuthorProof>(), proof);
}

#[test]
fn nonzero_game_exit_is_failed_and_releases_slot_for_a_fresh_request() {
    let (mut app, marker, proof) = app();
    install_completed_worker(&mut app, Ok(17));
    wait_for_completion(&mut app, 1);
    let status = app.world().resource::<NativePlayStatus>();
    assert_eq!(status.last_exit_code, Some(17));
    assert!(
        status
            .last_result
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap_err()
            .contains("17")
    );
    assert!(!status.active);
    assert_retry(&mut app, marker, &proof);
}

#[test]
fn worker_build_error_preserves_the_author_world_and_does_not_latch_a_retry_block() {
    let (mut app, marker, proof) = app();
    install_completed_worker(&mut app, Err("自有构建失败回执".into()));
    wait_for_completion(&mut app, 1);
    let status = app.world().resource::<NativePlayStatus>();
    assert_eq!(status.last_exit_code, None);
    assert_eq!(status.last_result, Some(Err("自有构建失败回执".into())));
    assert!(!status.active);
    assert!(!status.preparing);
    assert_retry(&mut app, marker, &proof);
}

#[test]
fn code_apply_queues_a_saved_launch_without_claiming_it_started() {
    let (mut app, marker, proof) = app();
    let project = app.world().resource::<NativePlaySettings>().project.clone();
    let prepared = std::sync::Arc::new(prepared_for_test(&project, default()));
    request_code_apply(app.world_mut(), prepared, 17).unwrap();
    let runtime = app.world().non_send::<NativePlayRuntime>();
    assert!(runtime.active.is_none());
    assert_eq!(runtime.code_apply_pending.as_ref().unwrap().request.map_id, None);
    assert_eq!(runtime.code_apply_status.as_ref().unwrap().state, CodeApplyState::Applying);
    assert!(app.world().get::<SameAuthorWorld>(marker).is_some());
    assert_eq!(app.world().resource::<AuthorProof>(), &proof);
    assert!(!cancel_pending_code_apply(app.world_mut(), 999, "旧请求"));
    assert!(cancel_pending_code_apply(app.world_mut(), 17, "源码已有更新"));
    assert!(app.world().non_send::<NativePlayRuntime>().code_apply_pending.is_none());
    assert_eq!(code_apply_status(app.world()).unwrap().state, CodeApplyState::Failed);
}

#[test]
fn native_backend_rejects_a_dx_candidate_before_queuing_a_standalone_restart() {
    let (mut app, marker, proof) = app();
    app.insert_resource(ProjectBuildBackend::Native);
    let project = app.world().resource::<NativePlaySettings>().project.clone();
    let prepared = std::sync::Arc::new(prepared_for_test(&project, default()));
    let error = request_code_apply(app.world_mut(), prepared, 21).unwrap_err();
    assert!(error.contains("构建方式"));
    let runtime = app.world().non_send::<NativePlayRuntime>();
    assert!(runtime.code_apply_pending.is_none());
    assert!(runtime.code_apply_status.is_none());
    assert!(runtime.active.is_none());
    assert_eq!(app.world().resource::<NativePlayStatus>().started, 1);
    assert!(app.world().get::<SameAuthorWorld>(marker).is_some());
    assert_eq!(app.world().resource::<AuthorProof>(), &proof);
}

#[test]
fn changing_backend_while_a_code_apply_is_pending_rejects_it_before_spawning() {
    let (mut app, marker, proof) = app();
    let project = app.world().resource::<NativePlaySettings>().project.clone();
    request_code_apply(app.world_mut(), std::sync::Arc::new(prepared_for_test(&project, default())), 22).unwrap();
    // 原UI运行门会拒绝此操作；在底层身份门模拟外部配置变化，确认迟到候选仍不能越过。
    app.insert_resource(ProjectBuildBackend::Native);
    code_apply::advance(app.world_mut());
    let runtime = app.world().non_send::<NativePlayRuntime>();
    assert!(runtime.code_apply_pending.is_none());
    assert!(runtime.active.is_none());
    let result = code_apply_status(app.world()).unwrap();
    assert_eq!((result.token, result.state), (22, CodeApplyState::Failed));
    assert!(result.error.unwrap().contains("构建方式"));
    assert_eq!(app.world().resource::<NativePlayStatus>().started, 1, "身份失效前不得产生新启动尝试");
    assert!(app.world().get::<SameAuthorWorld>(marker).is_some());
    assert_eq!(app.world().resource::<AuthorProof>(), &proof);
}

#[test]
fn code_apply_keeps_the_original_saved_map_until_the_old_worker_really_finishes() {
    let (mut app, marker, proof) = app();
    let (release, wait) = mpsc::sync_channel(1);
    let (_, progress_rx) = mpsc::sync_channel(4);
    let (_, result_rx) = mpsc::sync_channel(1);
    let control = crate::build_process::BuildControl::default();
    let mut runtime = app.world_mut().non_send_mut::<NativePlayRuntime>();
    runtime.active_request = Some(RunSavedProject { profile: default(), map_id: Some("saved-original".into()),
        has_unsaved_changes: true, stop_only: false });
    runtime.active = Some(resources::ActivePlay {
        control: control.clone(), progress_rx, result_rx,
        handle: thread::spawn(move || { let _ = wait.recv(); }),
    });
    let project = app.world().resource::<NativePlaySettings>().project.clone();
    request_code_apply(app.world_mut(), std::sync::Arc::new(prepared_for_test(&project, default())), 18).unwrap();
    let runtime = app.world().non_send::<NativePlayRuntime>();
    assert!(control.is_cancelled());
    assert!(runtime.active.is_some(), "取消意图不是进程或线程收口回执");
    let request = &runtime.code_apply_pending.as_ref().unwrap().request;
    assert_eq!(request.map_id.as_deref(), Some("saved-original"));
    assert!(request.has_unsaved_changes);
    assert!(app.world().get::<SameAuthorWorld>(marker).is_some());
    assert_eq!(app.world().resource::<AuthorProof>(), &proof);
    release.send(()).unwrap();
    app.world_mut().non_send_mut::<NativePlayRuntime>().active.take().unwrap().handle.join().unwrap();
}

#[test]
fn code_apply_needs_the_actual_spawn_notification_before_reporting_applied() {
    let (mut app, marker, proof) = app();
    app.world_mut().resource_mut::<NativePlayStatus>().preparing = true;
    let (_, progress_rx) = mpsc::sync_channel(4);
    let (_, result_rx) = mpsc::sync_channel(1);
    let (release, wait) = mpsc::sync_channel(1);
    let (started, started_rx) = mpsc::sync_channel(1);
    let mut runtime = app.world_mut().non_send_mut::<NativePlayRuntime>();
    runtime.active = Some(resources::ActivePlay { control: default(), progress_rx, result_rx,
        handle: thread::spawn(move || { let _ = wait.recv(); }) });
    runtime.code_apply_status = Some(CodeApplyStatus { token: 19, state: CodeApplyState::Applying, error: None });
    runtime.code_apply_active_token = Some(19);
    runtime.code_apply_started = Some(started_rx);
    app.update();
    assert_eq!(code_apply_status(app.world()).unwrap().state, CodeApplyState::Applying);
    assert!(app.world().resource::<NativePlayStatus>().preparing);
    started.send(()).unwrap();
    app.update();
    assert_eq!(code_apply_status(app.world()).unwrap().state, CodeApplyState::Applied);
    assert!(!app.world().resource::<NativePlayStatus>().preparing);
    assert!(app.world().resource::<NativePlayStatus>().active);
    assert!(app.world().get::<SameAuthorWorld>(marker).is_some());
    assert_eq!(app.world().resource::<AuthorProof>(), &proof);
    release.send(()).unwrap();
    app.world_mut().non_send_mut::<NativePlayRuntime>().active.take().unwrap().handle.join().unwrap();
}

const OWNED_LAUNCH_HELPER: &str = "FEATHRISE_OWNED_LAUNCH_LOADING_HELPER_A17E";

#[test]
fn completion_drains_created_ack_arriving_after_the_first_poll_without_hiding_exit_failure() {
    for final_result in [Ok(()), Err("自有进程退出码9".to_owned())] {
        let (started, started_rx) = mpsc::sync_channel(1);
        let mut runtime = NativePlayRuntime::default();
        runtime.code_apply_started = Some(started_rx);
        runtime.code_apply_active_token = Some(31);
        runtime.code_apply_status = Some(CodeApplyStatus { token: 31, state: CodeApplyState::Applying, error: None });
        let mut status = NativePlayStatus { preparing: true, active: true, ..default() };
        receive_process_created(&mut runtime, &mut status);
        assert_eq!(runtime.code_apply_status.as_ref().unwrap().state, CodeApplyState::Applying);
        assert!(status.preparing);
        // 确定性重现两次非阻塞检查之间的入队：首次为空，worker发送后真实join，再走生产完成分支。
        thread::spawn(move || started.send(()).unwrap()).join().unwrap();
        status.active = false;
        status.last_result = Some(final_result.clone());
        complete_code_apply(&mut runtime, &mut status);
        let apply = runtime.code_apply_status.as_ref().unwrap();
        match final_result {
            Ok(()) => { assert_eq!(apply.state, CodeApplyState::Applied); assert!(apply.error.is_none()); }
            Err(error) => { assert_eq!(apply.state, CodeApplyState::Failed); assert_eq!(apply.error.as_deref(), Some(error.as_str())); }
        }
        assert!(runtime.code_apply_started.is_none());
        assert!(runtime.code_apply_active_token.is_none());
        assert!(!status.preparing);
    }
}

#[test]
fn completion_without_created_ack_or_with_another_token_never_claims_the_wrong_application() {
    let (_, started_rx) = mpsc::sync_channel(1);
    let mut runtime = NativePlayRuntime::default();
    runtime.code_apply_started = Some(started_rx);
    runtime.code_apply_active_token = Some(32);
    runtime.code_apply_status = Some(CodeApplyStatus { token: 32, state: CodeApplyState::Applying, error: None });
    let mut status = NativePlayStatus { last_result: Some(Ok(())), ..default() };
    complete_code_apply(&mut runtime, &mut status);
    assert_eq!(runtime.code_apply_status.as_ref().unwrap().state, CodeApplyState::Failed);
    assert!(runtime.code_apply_status.as_ref().unwrap().error.as_ref().unwrap().contains("没有返回实际进程启动回执"));

    let (started, started_rx) = mpsc::sync_channel(1);
    started.send(()).unwrap();
    runtime.code_apply_started = Some(started_rx);
    runtime.code_apply_active_token = Some(32);
    runtime.code_apply_status = Some(CodeApplyStatus { token: 33, state: CodeApplyState::Applying, error: None });
    complete_code_apply(&mut runtime, &mut status);
    let apply = runtime.code_apply_status.as_ref().unwrap();
    assert_eq!((apply.token, apply.state), (33, CodeApplyState::Applying));
    assert!(apply.error.is_none());
}

#[test]
fn ordinary_preparing_ends_only_after_actual_owned_job_creation_and_not_after_game_exit() {
    use std::{io::Read, process::Command};
    let (mut app, marker, proof) = app();
    let (prepare, wait_prepare) = mpsc::sync_channel(1);
    let (started, started_rx) = mpsc::sync_channel(1);
    let (_, progress_rx) = mpsc::sync_channel(4);
    let (result, result_rx) = mpsc::sync_channel(1);
    let control = crate::build_process::BuildControl::default();
    let worker_control = control.clone();
    let handle = thread::spawn(move || {
        let outcome = (|| -> Result<u32, String> {
            wait_prepare.recv().map_err(|error| error.to_string())?;
            let mut command = Command::new(std::env::current_exe().map_err(|error| error.to_string())?);
            command.args(["--ignored", "--exact", "native_play::lifecycle_tests::owned_launch_loading_process_helper", "--nocapture", "--test-threads=1"])
                .env(OWNED_LAUNCH_HELPER, "owned-child");
            let output = crate::build_process::run_with_started(&command, &worker_control, None, Some(started), |mut file| {
                let mut buffer = [0u8; 8192];
                while file.read(&mut buffer).map_err(|error| error.to_string())? != 0 {}
                Ok(())
            })?;
            Ok(output.exit_code)
        })();
        let _ = result.send(outcome);
    });
    {
        let mut runtime = app.world_mut().non_send_mut::<NativePlayRuntime>();
        runtime.active = Some(resources::ActivePlay { control: control.clone(), progress_rx, result_rx, handle });
        runtime.launch_started = Some(started_rx);
    }
    app.world_mut().resource_mut::<NativePlayStatus>().preparing = true;
    // 失败也先取消本次精确 Job 并回收线程，再恢复断言；不遗留内部 helper 进程。
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        app.update();
        assert!(app.world().resource::<NativePlayStatus>().preparing, "后台准备不等于实际创建运行进程");
        prepare.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(6);
        while app.world().resource::<NativePlayStatus>().preparing {
            assert!(Instant::now() < deadline, "实际 Job 创建后没有结束启动准备状态");
            app.update();
            thread::yield_now();
        }
        let status = app.world().resource::<NativePlayStatus>();
        assert!(status.active && status.last_result.is_none(), "运行中不再显示启动准备动画");
        assert!(!app.world().non_send::<NativePlayRuntime>().active.as_ref().unwrap().handle.is_finished());
        assert!(app.world().get::<SameAuthorWorld>(marker).is_some());
        assert_eq!(app.world().resource::<AuthorProof>(), &proof);
    }));
    control.cancel();
    let _ = prepare.try_send(());
    wait_for_completion(&mut app, 1);
    assert!(!app.world().resource::<NativePlayStatus>().preparing);
    match outcome { Ok(()) => (), Err(panic) => std::panic::resume_unwind(panic) }
}

#[test]
fn cancelling_preparation_hides_loading_before_the_owned_worker_finishes() {
    let (mut app, _, _) = app();
    let (release, wait) = mpsc::sync_channel(1);
    let (_, progress_rx) = mpsc::sync_channel(4);
    let (result, result_rx) = mpsc::sync_channel(1);
    let control = crate::build_process::BuildControl::default();
    app.world_mut().non_send_mut::<NativePlayRuntime>().active = Some(resources::ActivePlay {
        control: control.clone(), progress_rx, result_rx,
        handle: thread::spawn(move || { let _ = wait.recv(); let _ = result.send(Err("自有准备已取消".into())); }),
    });
    app.world_mut().resource_mut::<NativePlayStatus>().preparing = true;
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        app.world_mut().write_message(RunSavedProject { profile: default(), map_id: None, has_unsaved_changes: false, stop_only: true });
        app.update();
        assert!(control.is_cancelled());
        assert!(!app.world().resource::<NativePlayStatus>().preparing);
        assert!(app.world().resource::<NativePlayStatus>().active, "撤销动画不冒充工作线程已结束");
    }));
    let _ = release.try_send(());
    wait_for_completion(&mut app, 1);
    match outcome { Ok(()) => (), Err(panic) => std::panic::resume_unwind(panic) }
}

#[test]
#[ignore = "仅供本模块通过专属 Job 创建的自有启动回执进程调用"]
fn owned_launch_loading_process_helper() {
    if std::env::var(OWNED_LAUNCH_HELPER).as_deref() != Ok("owned-child") { return; }
    loop { thread::park(); }
}

#[test]
fn code_apply_start_failure_keeps_drafts_and_reports_the_matching_token() {
    let (mut app, marker, proof) = app();
    install_completed_worker(&mut app, Err("自有新版本启动失败".into()));
    let (_, started_rx) = mpsc::sync_channel(1);
    let mut runtime = app.world_mut().non_send_mut::<NativePlayRuntime>();
    runtime.code_apply_status = Some(CodeApplyStatus { token: 20, state: CodeApplyState::Applying, error: None });
    runtime.code_apply_active_token = Some(20);
    runtime.code_apply_started = Some(started_rx);
    wait_for_completion(&mut app, 1);
    let status = code_apply_status(app.world()).unwrap();
    assert_eq!((status.token, status.state), (20, CodeApplyState::Failed));
    assert!(status.error.unwrap().contains("启动失败"));
    assert!(app.world().get::<SameAuthorWorld>(marker).is_some());
    assert_eq!(app.world().resource::<AuthorProof>(), &proof);
}
