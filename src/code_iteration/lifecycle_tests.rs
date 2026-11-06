//! 通过原关闭入口与真实持有线程复现取消收尾；不执行Cargo，也不写作者工程。
use super::{backend::Backend, resources::*, types::CodeIterationPhase as Phase};
use bevy::{ecs::system::RunSystemOnce, prelude::*, window::WindowCloseRequested};

fn project() -> crate::project::Project {
    let root = std::env::temp_dir().join(format!("code-close-recovery-{}", bevy::asset::uuid::Uuid::new_v4()));
    crate::project::Project {
        id: "close-recovery-fixture".into(), name: "取消关闭恢复夹具".into(),
        manifest: root.join("Cargo.toml"), package: "close_recovery_fixture".into(), root: root.clone(),
        context: crate::project::ProjectContext { root: root.clone(), id: "close-recovery-fixture".into(),
            name: "取消关闭恢复夹具".into(), format: 1, asset_roots: vec![root.join("assets")],
            editor_data: root.join(".editor"), storage_key: "close-recovery-fixture".into(), startup_scene: None },
    }
}

#[test]
fn original_close_cancel_restores_polling_while_the_old_worker_is_still_retiring() {
    let project = project();
    let profile = Default::default();
    let (backend, release) = Backend::held_for_test(&project, profile, 41, false);
    let mut app = App::new();
    app.add_plugins(crate::native_play::NativePlayPlugin::new(project.clone(), project.root.join("target")))
        .add_message::<WindowCloseRequested>().add_message::<AppExit>()
        .insert_resource(feathrise_editor_core::ProjectLaunchSettings::default())
        .insert_resource(CodeIterationState::new(&project))
        .insert_non_send(CodeIterationRuntime { backend: Some(backend), profile,
            settings: BackendSettings { project: project.clone(), target: project.root.join("target"), logs: None },
            lifecycle: BackendLifecycle::Running, last_generation: 1, failure: None })
        .add_systems(FixedUpdate, crate::native_play::request_exit);
    app.world_mut().write_message(WindowCloseRequested { window: Entity::PLACEHOLDER });
    app.world_mut().run_schedule(FixedUpdate);
    crate::native_project_close::tests::choose_discard(app.world_mut());
    app.world_mut().run_schedule(FixedUpdate);
    assert!(!app.world().non_send::<CodeIterationRuntime>().backend.as_ref().unwrap().is_finished());
    assert!(app.world().resource::<Messages<AppExit>>().is_empty());
    crate::native_project_close::cancel_workspace_close(app.world_mut());
    app.world_mut().run_schedule(FixedUpdate);
    assert!(!app.world().resource::<crate::native_project_close::NativeProjectCloseState>().active());
    app.world_mut().run_system_once(super::systems::poll).unwrap();
    let phase = app.world().resource::<CodeIterationState>().phase;
    let backend = app.world_mut().non_send_mut::<CodeIterationRuntime>().backend.take().unwrap();
    release.send(()).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while !backend.is_finished() && std::time::Instant::now() < deadline { std::thread::yield_now(); }
    backend.finish().unwrap();
    assert_eq!(phase, Phase::Waiting, "取消收尾后须恢复为等待旧任务退役，不能永远停在Stopping");
}

fn held_app(generation: u64, cleanup_failed: bool) -> (App, std::sync::mpsc::SyncSender<()>) {
    let project = project();
    let profile = Default::default();
    let (backend, release) = Backend::held_for_test(&project, profile, generation, cleanup_failed);
    let mut app = App::new();
    app.insert_resource(feathrise_editor_core::ProjectLaunchSettings::default())
        .insert_resource(crate::native_project_close::NativeProjectCloseState::default())
        .insert_resource(CodeIterationState::new(&project))
        .insert_non_send(CodeIterationRuntime { backend: Some(backend), profile,
            settings: BackendSettings { target: project.root.join("target"), project, logs: None },
            lifecycle: BackendLifecycle::Running, last_generation: 1, failure: None });
    (app, release)
}

fn wait_finished(app: &App) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let backend = app.world().non_send::<CodeIterationRuntime>().backend.as_ref().unwrap();
    while !backend.is_finished() && std::time::Instant::now() < deadline { std::thread::yield_now(); }
    assert!(backend.is_finished(), "自有持有线程放行后必须真实退役");
}

fn finish_current(app: &mut App) {
    if let Some(backend) = app.world_mut().non_send_mut::<CodeIterationRuntime>().backend.take() {
        backend.cancel();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !backend.is_finished() && std::time::Instant::now() < deadline { std::thread::yield_now(); }
        backend.finish().unwrap();
    }
}

#[test]
fn completed_retirement_keeps_its_final_generation_and_resumes_only_once() {
    let (mut app, release) = held_app(73, false);
    let identity = super::BuildIdentity { project_id: "close-recovery-fixture".into(),
        root: app.world().resource::<CodeIterationState>().project_root.clone(), profile: Default::default(), build_backend: Default::default(), backend_revision: 0,
        generation: 73, source_sha256: "已采用源码".into(), context_sha256: "原上下文".into() };
    app.world_mut().resource_mut::<CodeIterationState>().applied = Some(identity.clone());
    let marker = app.world_mut().spawn((Name::new("作者现场"), Transform::from_xyz(1., 2., 3.))).id();
    assert!(!super::ready_to_exit(app.world_mut()).unwrap());
    release.send(()).unwrap(); wait_finished(&app);
    assert!(super::ready_to_exit(app.world_mut()).unwrap());
    assert!(app.world().non_send::<CodeIterationRuntime>().backend.is_none());
    assert_eq!(app.world().non_send::<CodeIterationRuntime>().last_generation, 73);
    super::cancel_exit(app.world_mut());
    super::cancel_exit(app.world_mut());
    app.world_mut().run_system_once(super::systems::poll).unwrap();
    let runtime = app.world().non_send::<CodeIterationRuntime>();
    assert_eq!(runtime.lifecycle, BackendLifecycle::Running);
    let resumed_generation = runtime.backend.as_ref().unwrap().current_generation();
    assert!(resumed_generation > 73);
    assert!(!runtime.backend.as_ref().unwrap().is_current(&identity));
    super::cancel_exit(app.world_mut());
    assert_eq!(app.world().non_send::<CodeIterationRuntime>().backend.as_ref().unwrap().current_generation(), resumed_generation);
    assert_eq!(app.world().resource::<CodeIterationState>().applied.as_ref(), Some(&identity));
    assert_eq!(app.world().get::<Transform>(marker).unwrap().translation, Vec3::new(1., 2., 3.));
    finish_current(&mut app);
}

#[test]
fn another_confirmed_close_revokes_resume_before_a_second_backend_can_start() {
    let (mut app, release) = held_app(101, false);
    assert!(!super::ready_to_exit(app.world_mut()).unwrap());
    super::cancel_exit(app.world_mut());
    assert_eq!(app.world().non_send::<CodeIterationRuntime>().lifecycle, BackendLifecycle::Resuming);
    assert!(!super::ready_to_exit(app.world_mut()).unwrap());
    app.world_mut().run_system_once(super::systems::poll).unwrap();
    assert_eq!(app.world().non_send::<CodeIterationRuntime>().lifecycle, BackendLifecycle::Closing);
    assert_eq!(app.world().non_send::<CodeIterationRuntime>().backend.as_ref().unwrap().current_generation(), 101);
    release.send(()).unwrap(); wait_finished(&app);
    assert!(super::ready_to_exit(app.world_mut()).unwrap());
    assert!(app.world().non_send::<CodeIterationRuntime>().backend.is_none());
}

#[test]
fn failed_cleanup_and_exhausted_generations_never_spawn_a_replacement() {
    for (generation, cleanup_failed) in [(9, true), (u64::MAX, false)] {
        let (mut app, release) = held_app(generation, cleanup_failed);
        assert!(!super::ready_to_exit(app.world_mut()).unwrap());
        super::cancel_exit(app.world_mut());
        release.send(()).unwrap(); wait_finished(&app);
        app.world_mut().run_system_once(super::systems::poll).unwrap();
        assert!(app.world().non_send::<CodeIterationRuntime>().failure.is_some());
        assert!(app.world().non_send::<CodeIterationRuntime>().backend.is_none());
        assert_eq!(app.world().resource::<CodeIterationState>().phase, Phase::Failed);
        super::cancel_exit(app.world_mut());
        app.world_mut().run_system_once(super::systems::poll).unwrap();
        assert!(app.world().non_send::<CodeIterationRuntime>().backend.is_none());
    }
}

#[test]
fn ordinary_cancel_keeps_the_running_backend_and_idle_poll_does_not_dirty_ui() {
    let (mut app, release) = held_app(29, false);
    super::cancel_exit(app.world_mut());
    app.world_mut().clear_trackers();
    app.world_mut().run_system_once(super::systems::poll).unwrap();
    assert!(!app.world().get_resource_ref::<CodeIterationState>().unwrap().is_changed());
    assert_eq!(app.world().non_send::<CodeIterationRuntime>().lifecycle, BackendLifecycle::Running);
    assert_eq!(app.world().non_send::<CodeIterationRuntime>().backend.as_ref().unwrap().current_generation(), 29);
    release.send(()).unwrap(); wait_finished(&app); finish_current(&mut app);
}

#[test]
fn editor_3d_auto_build_keeps_the_selected_native_backend_across_mode_changes() {
    use crate::{native_play::ProjectBuildBackend, viewport_mode::{ViewportMode, ViewportModeState}};
    let (mut app, release) = held_app(29, false);
    app.insert_resource(ViewportModeState::default()).insert_resource(ProjectBuildBackend::Native);
    app.world_mut().run_system_once(super::systems::poll).unwrap();
    let project = &app.world().non_send::<CodeIterationRuntime>().settings.project;
    let identity = super::BuildIdentity { project_id: project.id.clone(), root: project.root.clone(),
        profile: Default::default(), build_backend: ProjectBuildBackend::Native, backend_revision: 1,
        generation: 29, source_sha256: "所选原生源码".into(), context_sha256: "原上下文".into() };
    assert!(app.world().non_send::<CodeIterationRuntime>().backend.as_ref().unwrap().is_current(&identity),
        "编辑器3D的自动编译必须采用顶栏的原生选择");
    for mode in [ViewportMode::EmbeddedWorld, ViewportMode::SavedWorld] {
        app.world_mut().resource_mut::<ViewportModeState>().selected = mode;
        app.world_mut().run_system_once(super::systems::poll).unwrap();
        let backend = app.world().non_send::<CodeIterationRuntime>().backend.as_ref().unwrap();
        assert!(backend.is_current(&identity), "切换呈现方式不能恢复DX或撤销同后端候选");
        assert_eq!(backend.current_generation(), 29, "只改呈现方式不能假装保存源码重新编译");
    }
    release.send(()).unwrap(); wait_finished(&app); finish_current(&mut app);
}

#[test]
fn retained_close_message_blocks_only_its_new_frame_and_not_a_later_apply() {
    for same_frame in [false, true] {
        let (mut app, release) = held_app(31, false);
        let project = app.world().non_send::<CodeIterationRuntime>().settings.project.clone();
        let version = std::sync::Arc::new(super::PreparedVersion {
            identity: super::BuildIdentity { project_id: project.id.clone(), root: project.root.clone(),
                profile: Default::default(), build_backend: Default::default(), backend_revision: 0, generation: 30,
                source_sha256: "旧候选".into(), context_sha256: "原上下文".into() },
            artifact: super::PreparedArtifact::Restart(std::sync::Arc::new(crate::native_play::prepared_for_test(&project, Default::default()))),
        });
        let mut state = app.world_mut().resource_mut::<CodeIterationState>();
        state.generation = 30;
        state.pending = Some(version);
        state.phase = Phase::Ready;
        drop(state);
        // 显式保留原关闭队列，复现实窗取消后 is_empty 始终为 false 的情况。
        app.init_resource::<Messages<WindowCloseRequested>>()
            .add_message::<super::RequestApplyCode>()
            .add_systems(Update, super::systems::apply);
        app.world_mut().write_message(WindowCloseRequested { window: Entity::PLACEHOLDER });
        if same_frame {
            app.world_mut().write_message(super::RequestApplyCode { generation: 30 });
        }
        app.update();
        assert!(app.world().resource::<CodeIterationState>().pending.is_some(),
            "新关闭与应用同帧时必须仍拦截应用，不能清除候选或启动运行");
        assert!(!app.world().resource::<Messages<WindowCloseRequested>>().is_empty(),
            "各消费者使用独立游标，不清共享关闭队列冒充修复");
        app.update();
        assert!(app.world().resource::<CodeIterationState>().pending.is_some(),
            "关闭帧丢弃的旧应用点击不能在下一帧自动重放");
        app.world_mut().write_message(super::RequestApplyCode { generation: 30 });
        app.update();
        let state = app.world().resource::<CodeIterationState>();
        // 真正进入原应用身份门才会拒收30代旧候选；这不是直接写应用成功或启动夹具进程。
        assert!(state.pending.is_none(), "已取消的旧关闭消息不能吞掉下一次应用请求");
        assert_eq!(state.phase, Phase::Waiting);
        release.send(()).unwrap(); wait_finished(&app); finish_current(&mut app);
    }
}
