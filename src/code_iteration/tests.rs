//! 编译完成值的身份门与失败保留回归；实际编译、按钮定位和游戏行为由显式窗口夹具验证。

use super::{resources::CodeIterationState, systems::adopt_event, types::CodeIterationPhase as Phase, BuildIdentity, Event};
use crate::compiler_diagnostics::{CompilerDiagnostic, DiagnosticLevel};
use bevy::{ecs::system::RunSystemOnce, prelude::{App, World}};
use std::{fs, path::PathBuf, sync::{Arc, mpsc}, time::{Duration, Instant}};

fn project() -> crate::project::Project {
    let root = PathBuf::from("C:/code-iteration-state-fixture");
    crate::project::Project {
        id: "fixture-code".into(), name: "代码状态回归".into(), manifest: root.join("Cargo.toml"),
        package: "fixture-code".into(), root: root.clone(),
        context: crate::project::ProjectContext { root: root.clone(), id: "fixture-code".into(), name: "代码状态回归".into(),
            format: 1, asset_roots: vec![root.join("assets")], editor_data: root.join(".editor"), storage_key: "fixture-code".into(), startup_scene: None },
    }
}

fn identity(generation: u64) -> BuildIdentity {
    let project = project();
    BuildIdentity { project_id: project.id, root: project.root, profile: default_profile(), build_backend: Default::default(), backend_revision: 0, generation,
        source_sha256: format!("source-{generation}"), context_sha256: "fixture-context".into() }
}

fn default_profile() -> feathrise_editor_core::ProjectBuildProfile { Default::default() }

#[test]
fn plugin_modal_and_release_frame_discard_code_buttons_until_a_new_explicit_click() {
    use super::{components::{ApplyVersion, CodeIterationAction}, RequestApplyCode};
    use crate::native_plugins::management::PluginManagerState;
    use bevy::prelude::*;
    use feathrise_editor_ui::EditorButtonClick;

    let project = project();
    let mut app = App::new();
    let mut manager = PluginManagerState::new(project.root.clone());
    manager.open = true;
    app.insert_resource(CodeIterationState::new(&project)).insert_resource(manager)
        .add_message::<RequestApplyCode>().add_systems(Update, super::ui::input);
    // 只投递原按钮已确认的回执，执行生产input；不启动编译或伪造版本采用成功。
    let diagnostics = app.world_mut().spawn((CodeIterationAction::Diagnostics, ApplyVersion::default(), EditorButtonClick::triggered())).id();
    let apply = app.world_mut().spawn((CodeIterationAction::Apply, ApplyVersion(Some(42)), EditorButtonClick::triggered())).id();
    app.update();
    assert!(!app.world().resource::<CodeIterationState>().diagnostics_open);
    assert!(app.world().resource::<Messages<RequestApplyCode>>().is_empty());

    {
        let mut manager = app.world_mut().resource_mut::<PluginManagerState>();
        manager.open = false;
        manager.captured = true;
    }
    for entity in [diagnostics, apply] {
        app.world_mut().entity_mut(entity).insert(EditorButtonClick::triggered());
    }
    app.update();
    assert!(!app.world().resource::<CodeIterationState>().diagnostics_open);
    assert!(app.world().resource::<Messages<RequestApplyCode>>().is_empty(), "关闭释放帧不能请求重启预览");

    app.world_mut().resource_mut::<PluginManagerState>().captured = false;
    app.update();
    assert!(!app.world().resource::<CodeIterationState>().diagnostics_open);
    assert!(app.world().resource::<Messages<RequestApplyCode>>().is_empty(), "被模态消费的旧点击不能在归还输入后重放");

    app.world_mut().entity_mut(diagnostics).insert(EditorButtonClick::triggered());
    app.update();
    assert!(app.world().resource::<CodeIterationState>().diagnostics_open, "归还输入后的新诊断点击应正常受理");
    app.world_mut().entity_mut(apply).insert(EditorButtonClick::triggered());
    app.update();
    let mut cursor = bevy::ecs::message::MessageCursor::<RequestApplyCode>::default();
    let requests = cursor.read(app.world().resource::<Messages<RequestApplyCode>>()).map(|request| request.generation).collect::<Vec<_>>();
    assert_eq!(requests, [42], "只有归还输入后的新应用点击发出一次冻结版本的意图");
}

#[test]
fn late_build_error_does_not_replace_a_newer_status_or_diagnostics() {
    let mut state = CodeIterationState::new(&project());
    adopt_event(&mut state, Event::Building { identity: identity(5) }, true);
    let status = state.status.clone();
    adopt_event(&mut state, Event::Finished { identity: identity(4), result: Err("旧错误".into()), diagnostics: vec![] }, false);
    assert_eq!(state.generation, 5);
    assert_eq!(state.phase, Phase::Building);
    assert_eq!(state.status, status);
}

#[test]
fn wrong_project_result_never_changes_the_current_editor_state() {
    let mut state = CodeIterationState::new(&project());
    let before = state.status.clone();
    let mut wrong = identity(4);
    wrong.project_id = "other-project".into();
    adopt_event(&mut state, Event::Finished { identity: wrong, result: Err("另一工程的错误".into()), diagnostics: vec![] }, true);
    assert_eq!(state.phase, Phase::Watching);
    assert_eq!(state.status, before);
}

#[test]
fn compiler_failure_keeps_the_last_actually_applied_identity() {
    let mut state = CodeIterationState::new(&project());
    state.applied = Some(identity(2));
    let diagnostic = CompilerDiagnostic { level: DiagnosticLevel::Error, code: Some("E0308".into()), message: "类型不匹配".into(), location: None };
    adopt_event(&mut state, Event::Finished { identity: identity(3), result: Err("编译错误".into()), diagnostics: vec![diagnostic.clone()] }, true);
    assert_eq!(state.applied, Some(identity(2)));
    assert_eq!(state.phase, Phase::Failed);
    assert_eq!(state.diagnostics.as_slice(), &[diagnostic]);
    assert!(state.pending.is_none());
}

#[test]
fn new_save_clears_stale_diagnostics_without_clearing_the_applied_version() {
    let mut state = CodeIterationState::new(&project());
    state.applied = Some(identity(2));
    state.diagnostics = std::sync::Arc::new(vec![CompilerDiagnostic { level: DiagnosticLevel::Error, message: "旧诊断".into(), code: None, location: None }]);
    adopt_event(&mut state, Event::Waiting { generation: 4, backend_revision: 0 }, true);
    assert!(state.diagnostics.is_empty());
    assert_eq!(state.applied, Some(identity(2)));
    assert_eq!(state.phase, Phase::Waiting);
    assert_eq!(state.diagnostic_revision, 1);
}

/// 独占工程只供新Backend建立真实监听基线；旧后台的两个真实线程分别由门控制，不运行Cargo。
struct RecoveryFixture {
    app: App,
    root: PathBuf,
    control: crate::build_process::BuildControl,
    source_release: Option<mpsc::SyncSender<()>>,
    build_release: Option<mpsc::SyncSender<()>>,
    source_end: mpsc::Receiver<()>,
    build_end: mpsc::Receiver<()>,
}

impl RecoveryFixture {
    fn new() -> Self {
        use super::resources::{BackendLifecycle, BackendSettings, CodeIterationRuntime};
        let root = std::env::temp_dir().join(format!("feathrise-code-recovery-tests-{}", bevy::asset::uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let root = root.canonicalize().unwrap();
        fs::create_dir(root.join("src")).unwrap();
        fs::create_dir(root.join("assets")).unwrap();
        fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
        fs::write(root.join("Cargo.toml"), "[package]\nname='fixture_game'\nversion='0.1.0'\nedition='2024'\n").unwrap();
        fs::write(root.join("editor-project.json"), serde_json::to_vec(&serde_json::json!({
            "format":2,"id":"845f04e0-1234-4abc-8def-123456789abc","name":"关闭取消恢复夹具","package":"fixture_game",
            "cargo_manifest":"Cargo.toml","asset_roots":["assets"],"editor_data":".editor",
            "game_entry":{"kind":"saved-world","version":1}
        })).unwrap()).unwrap();
        let project = crate::project::open(&root).unwrap();
        let control = crate::build_process::BuildControl::default();
        let (source_release, source_gate) = mpsc::sync_channel(1);
        let (build_release, build_gate) = mpsc::sync_channel(1);
        let (backend, source_end, build_end) = super::Backend::controlled_for_test(&project,
            default_profile(), control.clone(), source_gate, build_gate);
        let mut app = App::new();
        app.insert_resource(CodeIterationState::new(&project))
            .init_resource::<feathrise_editor_core::ProjectLaunchSettings>()
            .init_resource::<crate::native_project_close::NativeProjectCloseState>()
            .add_message::<feathrise_editor_map::standalone::SavedPlayFeedback>()
            .insert_non_send(CodeIterationRuntime { backend: Some(backend), profile: default_profile(),
                settings: BackendSettings { project, target: root.join("target"), logs: None },
                lifecycle: BackendLifecycle::Running, last_generation: 1, failure: None });
        Self { app, root, control, source_release: Some(source_release), build_release: Some(build_release), source_end, build_end }
    }

    fn authorize_close(&mut self) {
        assert!(!crate::native_project_close::authorize_exit(self.app.world_mut(), true));
        crate::native_project_close::tests::choose_discard(self.app.world_mut());
        assert!(crate::native_project_close::authorize_exit(self.app.world_mut(), false));
        assert!(!super::ready_to_exit(self.app.world_mut()).unwrap());
        assert!(self.control.is_cancelled());
    }

    fn cancel_close(&mut self) {
        crate::native_project_close::tests::choose_cancel(self.app.world_mut());
        assert!(!crate::native_project_close::authorize_exit(self.app.world_mut(), false));
        assert!(!self.app.world().resource::<crate::native_project_close::NativeProjectCloseState>().active());
    }

    fn recover(&mut self) {
        // 与main唯一恢复入口一致：先撤回退出意图，再由原poll等待真实退役及补偿源码首扫。
        super::cancel_exit(self.app.world_mut());
        self.app.world_mut().run_system_once(super::systems::poll).unwrap();
    }

    fn release_source(&mut self) {
        self.source_release.take().unwrap().send(()).unwrap();
        self.source_end.recv_timeout(Duration::from_secs(2)).unwrap();
    }

    fn release_build(&mut self) {
        self.build_release.take().unwrap().send(()).unwrap();
        self.build_end.recv_timeout(Duration::from_secs(2)).unwrap();
    }

    fn wait_old_finished(&self) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while !self.app.world().non_send::<super::resources::CodeIterationRuntime>().backend.as_ref().unwrap().is_finished() {
            assert!(Instant::now() < deadline, "自有旧后台线程未结束");
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

impl Drop for RecoveryFixture {
    fn drop(&mut self) {
        if let Some(release) = self.source_release.take() { let _ = release.try_send(()); }
        if let Some(release) = self.build_release.take() { let _ = release.try_send(()); }
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let runtime = self.app.world().non_send::<super::resources::CodeIterationRuntime>();
            let Some(backend) = runtime.backend.as_ref() else { break; };
            backend.cancel();
            if backend.is_finished() {
                if let Some(backend) = self.app.world_mut().non_send_mut::<super::resources::CodeIterationRuntime>().backend.take() {
                    let _ = backend.finish();
                }
                break;
            }
            if Instant::now() >= deadline { return; }
            std::thread::sleep(Duration::from_millis(1));
        }
        // 逐项清理本轮明确创建的普通文件/空目录；任何额外内容均保留，不执行递归删除。
        let owned = self.root.file_name().and_then(|name| name.to_str())
            .and_then(|name| name.strip_prefix("feathrise-code-recovery-tests-"))
            .is_some_and(|id| bevy::asset::uuid::Uuid::parse_str(id).is_ok());
        if !owned || self.root.canonicalize().ok().as_ref() != Some(&self.root) { return; }
        for path in ["src/main.rs", "Cargo.toml", "editor-project.json"] { let _ = fs::remove_file(self.root.join(path)); }
        for path in ["src", "assets", "target", ".editor"] { let _ = fs::remove_dir(self.root.join(path)); }
        let _ = fs::remove_dir(&self.root);
    }
}

#[test]
fn cancelling_close_waits_for_both_old_threads_and_restores_a_new_input_lease() {
    use super::resources::{BackendLifecycle, CodeIterationRuntime};
    let mut fixture = RecoveryFixture::new();
    let old_guard = super::export_input_guard(fixture.app.world()).unwrap();
    assert!((old_guard.0)());
    let project = fixture.app.world().non_send::<CodeIterationRuntime>().settings.project.clone();
    let identity = BuildIdentity { project_id: project.id.clone(), root: project.root.clone(), profile: default_profile(), build_backend: Default::default(), backend_revision: 0,
        generation: 1, source_sha256: "old-source".into(), context_sha256: "old-context".into() };
    let version = Arc::new(super::PreparedVersion { identity: identity.clone(),
        artifact: super::PreparedArtifact::Restart(Arc::new(crate::native_play::prepared_for_test(&project, default_profile()))) });
    {
        let mut state = fixture.app.world_mut().resource_mut::<CodeIterationState>();
        state.pending = Some(version.clone());
        // 代码应用与新关闭在main沿原guard互斥；此处只放未采用候选，避免构造两种事务同时拥有权。
        state.applied = Some(identity.clone());
        state.generation = 1;
        state.diagnostics = Arc::new(vec![CompilerDiagnostic { level: DiagnosticLevel::Error,
            message: "旧后台诊断".into(), code: None, location: None }]);
    }
    let marker = fixture.app.world_mut().spawn_empty().id();
    fixture.authorize_close();
    assert!(!(old_guard.0)());
    fixture.recover();
    assert_eq!(fixture.app.world().non_send::<CodeIterationRuntime>().lifecycle, BackendLifecycle::Closing, "关闭仍active时不能恢复");
    fixture.cancel_close();
    fixture.recover();
    assert!(super::export_blocker(fixture.app.world()).is_some());
    assert!(fixture.app.world().resource::<CodeIterationState>().pending.is_none());
    assert!(fixture.app.world().resource::<CodeIterationState>().applying.is_none());
    fixture.release_source();
    fixture.recover();
    assert_eq!(fixture.app.world().non_send::<CodeIterationRuntime>().lifecycle, BackendLifecycle::Resuming, "第二个真实线程仍占槽时不能重开后台");
    assert!(super::export_blocker(fixture.app.world()).is_some());
    fixture.release_build();
    fixture.wait_old_finished();
    fixture.recover();
    let runtime = fixture.app.world().non_send::<CodeIterationRuntime>();
    assert_eq!(runtime.lifecycle, BackendLifecycle::Running);
    assert!(runtime.failure.is_none());
    assert!(runtime.backend.is_some());
    assert!(super::export_blocker(fixture.app.world()).is_none());
    assert!((super::export_input_guard(fixture.app.world()).unwrap().0)());
    assert!(!(old_guard.0)(), "新后台不能复活旧Shared的导出租约");
    let state = fixture.app.world().resource::<CodeIterationState>();
    assert_ne!(state.phase, Phase::Stopping);
    assert!(state.generation > identity.generation, "恢复补偿首扫不能复用旧后台代次");
    assert!(state.pending.is_none() && state.applying.is_none() && state.diagnostics.is_empty());
    assert_eq!(state.applied.as_ref(), Some(&identity));
    assert_eq!(state.diagnostic_revision, 1);
    assert!(fixture.app.world().get_entity(marker).is_ok());
    assert_eq!(fixture.app.world().non_send::<CodeIterationRuntime>().settings.target, fixture.root.join("target"));
}

#[test]
fn active_or_missing_close_state_cannot_restart_a_drained_backend() {
    use super::resources::{BackendLifecycle, CodeIterationRuntime};
    let mut fixture = RecoveryFixture::new();
    fixture.authorize_close();
    fixture.release_source();
    fixture.release_build();
    fixture.wait_old_finished();
    fixture.recover();
    assert!(fixture.app.world().non_send::<CodeIterationRuntime>().backend.as_ref().unwrap().is_finished());
    assert_eq!(fixture.app.world().non_send::<CodeIterationRuntime>().lifecycle, BackendLifecycle::Closing);
    assert!(super::ready_to_exit(fixture.app.world_mut()).unwrap());
    assert!(fixture.app.world().non_send::<CodeIterationRuntime>().backend.is_none());
    fixture.app.world_mut().remove_resource::<crate::native_project_close::NativeProjectCloseState>();
    fixture.recover();
    assert!(fixture.app.world().non_send::<CodeIterationRuntime>().backend.is_none());
    assert_eq!(fixture.app.world().non_send::<CodeIterationRuntime>().lifecycle, BackendLifecycle::Closing);
    assert!(super::export_blocker(fixture.app.world()).is_some());
}

#[test]
fn cleanup_failure_survives_close_cancel_and_never_reopens_build_or_export() {
    use super::resources::{BackendLifecycle, CodeIterationRuntime};
    let mut fixture = RecoveryFixture::new();
    fixture.authorize_close();
    fixture.control.mark_cleanup_failed();
    fixture.cancel_close();
    fixture.release_source();
    fixture.release_build();
    fixture.wait_old_finished();
    fixture.recover();
    let failure = fixture.app.world().non_send::<CodeIterationRuntime>().failure.clone().unwrap();
    for _ in 0..3 { fixture.recover(); }
    fixture.app.add_systems(bevy::prelude::Update, super::systems::poll);
    fixture.app.update();
    let runtime = fixture.app.world().non_send::<CodeIterationRuntime>();
    assert_ne!(runtime.lifecycle, BackendLifecycle::Running);
    assert!(runtime.backend.is_none());
    assert_eq!(runtime.failure.as_ref(), Some(&failure));
    assert_eq!(fixture.app.world().resource::<CodeIterationState>().phase, Phase::Failed);
    assert!(super::export_blocker(fixture.app.world()).unwrap().contains(&failure));
    assert!(super::ready_to_exit(fixture.app.world_mut()).is_err());
}

#[test]
fn apps_without_code_iteration_keep_the_existing_export_contract() {
    let world = World::new();
    assert!(super::export_blocker(&world).is_none());
    assert!(super::export_input_guard(&world).is_none());
}
