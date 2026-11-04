//! 原地采用回归驱动实际宿主事务；不启动Cargo或运行工程，通道代表唯一可靠控制槽。

use super::*;
use super::super::resources::{Active, Event};
use crate::{build_artifacts, build_process::BuildControl, code_iteration::SourceSnapshot};
use feathrise_editor_protocol::SessionIdentity;
use std::{collections::BTreeSet, fs, path::PathBuf, thread};

struct Fixture {
    world: World,
    runtime: Runtime,
    status: EmbeddedStatus,
    release: mpsc::SyncSender<()>,
    root: PathBuf,
    camera: Entity,
}

fn fixture(sent: bool) -> Fixture {
    let root = std::env::temp_dir().join(format!("feathrise-function-transaction-{}", bevy::asset::uuid::Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    let root = root.canonicalize().unwrap();
    // 只验证不可变租约和事务，文件不执行；publish真实创建受控目录和摘要清单。
    let executable = root.join("unexecuted-fixture.exe");
    fs::write(&executable, b"unexecuted-owned-transaction-fixture").unwrap();
    let bundle = build_artifacts::publish(&root, "owned-function-transaction", &executable, &[], serde_json::json!({}), &BuildControl::default()).unwrap();
    let baseline = Arc::new(FatBaseline { client_json: Arc::new(Vec::new()),
        source: SourceSnapshot { sha256: "body-one".into(), environment_sha256: "same-env".into(), hotpatch_shape: Some("same-shape".into()),
            files: 1, bytes: 16, watches: BTreeSet::new(), source_watches: BTreeSet::new() }, base_build: "fat-base".into(),
        directory: bundle.directory.clone(), patch_root: bundle.directory.join("patches"), features: "function_hotpatch".into(), bundle });
    let target = PatchTarget { project_id: "owned-function-transaction".into(), session: 17, pid: 19, world_id: 23,
        aslr_reference: 29, base_build: baseline.base_build.clone() };
    let prepared = Arc::new(PreparedPatch { target: target.clone(), source_generation: 5, artifact: "patch-5/package.json".into(),
        sha256: "a".repeat(64), baseline: baseline.clone() });
    let (sender, _commands) = mpsc::sync_channel(1);
    let (_events, receiver) = mpsc::sync_channel::<Event>(1);
    let (_finished, result) = mpsc::sync_channel(1);
    let (release, wait) = mpsc::sync_channel(1);
    let mut runtime = Runtime::default();
    runtime.identity = Some(SessionIdentity { project_id: target.project_id.clone(), session: 17, generation: 3 });
    runtime.active = Some(Active { control: default(), sender, receiver, result,
        thread: thread::spawn(move || { let _ = wait.recv(); }) });
    runtime.hotpatch.baseline = Some(baseline);
    runtime.hotpatch.ready = Some(target);
    runtime.hotpatch.flight = Some(HotpatchFlight { prepared, token: 5, started: Instant::now(),
        sent: sent.then_some((3, Instant::now())), timeout_reported: false });
    runtime.code_apply_status = Some(CodeApplyStatus { token: 5, state: CodeApplyState::Applying, error: None });
    let mut world = World::new();
    world.init_resource::<ViewportModeState>();
    world.init_resource::<Messages<feathrise_editor_map::standalone::SavedPlayFeedback>>();
    let camera = world.spawn(Transform::from_xyz(7.0, 8.0, 9.0)).id();
    Fixture { world, runtime, status: EmbeddedStatus { active: true, ready: true, pid: 19, world_id: 23,
        frames: 101, ticks: 89, starts: 1, ..default() }, release, root, camera }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(active) = self.runtime.active.take() { let _ = self.release.send(()); let _ = active.thread.join(); }
        // 只清理本测试实际创建并确认位于系统临时目录的UUID根，不扫描或移除其他工程。
        let temporary = std::env::temp_dir().canonicalize().unwrap();
        if self.root.parent() == Some(temporary.as_path())
            && self.root.file_name().unwrap().to_string_lossy().starts_with("feathrise-function-transaction-") {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}

fn acknowledge(fixture: &mut Fixture, patch_id: u64, source_generation: u64, pid: u32, world_id: u64, epoch: u64, applied: bool) {
    result(&mut fixture.world, &mut fixture.runtime, &mut fixture.status, patch_id, source_generation,
        "fat-base".into(), pid, world_id, epoch, applied, (!applied).then(|| "受控夹具拒绝补丁".into()));
}

#[test]
fn only_exact_real_result_adopts_epoch_and_keeps_author_and_runtime_identity() {
    let mut fixture = fixture(true);
    for (patch_id, generation, pid, world_id) in [(4, 5, 19, 23), (5, 4, 19, 23), (5, 5, 20, 23), (5, 5, 19, 24)] {
        acknowledge(&mut fixture, patch_id, generation, pid, world_id, 1, true);
        assert!(fixture.runtime.hotpatch.flight.is_some());
        assert_eq!(fixture.status.patch_epoch, 0);
    }
    fixture.runtime.identity.as_mut().unwrap().generation = 4;
    acknowledge(&mut fixture, 5, 5, 19, 23, 1, true);
    assert!(fixture.runtime.hotpatch.flight.is_some());
    fixture.runtime.identity.as_mut().unwrap().generation = 3;
    acknowledge(&mut fixture, 5, 5, 19, 23, 1, true);
    assert!(fixture.runtime.hotpatch.flight.is_none());
    assert_eq!(fixture.runtime.code_apply_status.as_ref().unwrap().state, CodeApplyState::Applied);
    assert_eq!((fixture.status.pid, fixture.status.world_id, fixture.status.patch_epoch, fixture.status.starts), (19, 23, 1, 1));
    assert_eq!((fixture.status.frames, fixture.status.ticks), (101, 89));
    assert_eq!(fixture.world.get::<Transform>(fixture.camera).unwrap().translation, Vec3::new(7.0, 8.0, 9.0));
    assert!(!fixture.runtime.active.as_ref().unwrap().control.is_cancelled());
}

#[test]
fn refusal_is_a_code_result_and_never_breaks_the_healthy_viewport() {
    let mut fixture = fixture(true);
    acknowledge(&mut fixture, 5, 5, 19, 23, 0, false);
    assert_eq!(fixture.runtime.code_apply_status.as_ref().unwrap().state, CodeApplyState::Failed);
    assert!(fixture.runtime.hotpatch.flight.is_none());
    assert!(fixture.status.error.is_none() && fixture.status.ready && fixture.status.active);
    assert_eq!(fixture.status.patch_epoch, 0);
    assert!(!fixture.runtime.active.as_ref().unwrap().control.is_cancelled());
}

#[test]
fn sent_timeout_keeps_the_single_flight_and_accepts_a_late_actual_result() {
    let mut fixture = fixture(true);
    fixture.runtime.hotpatch.flight.as_mut().unwrap().sent = Some((3, Instant::now() - Duration::from_secs(31)));
    advance(&mut fixture.world, &mut fixture.runtime, &mut fixture.status, ViewportMode::EmbeddedWorld, ProjectBuildProfile::Debug);
    assert!(fixture.runtime.hotpatch.flight.as_ref().unwrap().timeout_reported);
    assert_eq!(fixture.runtime.code_apply_status.as_ref().unwrap().state, CodeApplyState::Applying);
    assert!(!fixture.runtime.active.as_ref().unwrap().control.is_cancelled());
    acknowledge(&mut fixture, 5, 5, 19, 23, 1, true);
    assert_eq!(fixture.status.patch_epoch, 1);
}

#[test]
fn new_source_cancels_only_the_intent_before_it_enters_the_control_queue() {
    let mut fixture = fixture(false);
    fixture.world.insert_non_send(std::mem::take(&mut fixture.runtime));
    assert!(cancel_pending(&mut fixture.world, 5, "已有较新保存"));
    fixture.runtime = fixture.world.remove_non_send::<Runtime>().unwrap();
    assert!(fixture.runtime.hotpatch.flight.is_none());
    assert!(!fixture.runtime.active.as_ref().unwrap().control.is_cancelled());
    let mut sent = self::fixture(true);
    sent.world.insert_non_send(std::mem::take(&mut sent.runtime));
    assert!(!cancel_pending(&mut sent.world, 5, "已有较新保存"));
    sent.runtime = sent.world.remove_non_send::<Runtime>().unwrap();
    assert!(sent.runtime.hotpatch.flight.is_some());
}

#[test]
fn incorrect_epoch_locks_further_transactions_without_assuming_a_rollback() {
    let mut fixture = fixture(true);
    acknowledge(&mut fixture, 5, 5, 19, 23, 2, true);
    assert!(fixture.runtime.hotpatch.flight.as_ref().unwrap().timeout_reported);
    assert_eq!(fixture.status.patch_epoch, 0);
    assert_eq!(fixture.runtime.code_apply_status.as_ref().unwrap().state, CodeApplyState::Applying);
    assert!(fixture.status.error.is_none());
    assert!(!fixture.runtime.active.as_ref().unwrap().control.is_cancelled());
}
