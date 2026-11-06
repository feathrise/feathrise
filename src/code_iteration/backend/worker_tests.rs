//! 原生保存事件与单槽合并的真实文件夹具；不调用 Cargo，不写用户工程。

use super::*;
use crate::build_process::BuildControl;
use std::{
    fs,
    sync::{
        Condvar,
        atomic::{AtomicBool, AtomicU8, AtomicU64},
    },
};

struct Fixture {
    directory: PathBuf,
    project: Project,
}

impl Fixture {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "feathrise-own-watch-{}",
            bevy::asset::uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(directory.join("src")).unwrap();
        fs::create_dir(directory.join("assets")).unwrap();
        fs::write(
            directory.join("Cargo.toml"),
            "[package]\nname='owned_watch_fixture'\nversion='0.1.0'\n",
        )
        .unwrap();
        fs::write(directory.join("src/main.rs"), "fn main() {}\n").unwrap();
        fs::write(directory.join("editor-project.json"), r#"{"format":1,"id":"owned-watch","name":"监听夹具","cargo_manifest":"Cargo.toml","package":"owned_watch_fixture"}"#).unwrap();
        let project = crate::project::open(&directory).unwrap();
        Self { directory, project }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

fn shared() -> Arc<Shared> {
    shared_at(1)
}

fn shared_at(generation: u64) -> Arc<Shared> {
    Arc::new(Shared {
        control: BuildControl::default(),
        generation: AtomicU64::new(generation),
        profile: AtomicU8::new(0),
        build_backend: AtomicU64::new(0),
        stopped: AtomicBool::new(false),
        latest: Mutex::new(None),
        wake: Condvar::new(),
        output_scope: Arc::default(),
    })
}

#[test]
fn same_path_recursive_watch_wins_over_nonrecursive_package_parent() {
    let path = PathBuf::from("D:/owned/src");
    let watches = BTreeSet::from([(path.clone(), false), (path.clone(), true)]);
    let normalized = normalized_watches(&watches, std::path::Path::new("D:/owned"));
    assert!(normalized.contains(&(path.clone(), true)));
    assert!(!normalized.contains(&(path, false)));
}

#[test]
fn second_capture_with_a_new_dependency_cannot_publish_an_unwatched_baseline_or_request() {
    let fixture = Fixture::new();
    let dependency_a = Fixture::new();
    let dependency_b = Fixture::new();
    fs::write(&fixture.project.manifest, format!(
        "[package]\nname='owned_watch_fixture'\nversion='0.1.0'\n[dependencies]\na={{package='owned_sdk_a',path='{}'}}\n",
        dependency_a.project.root.display())).unwrap();
    fs::write(&dependency_a.project.manifest,
        "[package]\nname='owned_sdk_a'\nversion='0.1.0'\n").unwrap();
    fs::write(dependency_a.project.root.join("src/lib.rs"), "pub fn a() {}\n").unwrap();
    fs::write(&dependency_b.project.manifest,
        "[package]\nname='owned_sdk_b'\nversion='0.1.0'\n").unwrap();
    fs::write(dependency_b.project.root.join("src/lib.rs"), "pub fn b() {}\n").unwrap();
    let target = fixture.project.root.join("out-cache");
    let control = BuildControl::default();
    let first = source::capture(&fixture.project, &target, &control).unwrap();
    let installed_a = normalized_watches(&first.watches, &fixture.project.root);
    let scope_a = EventScope::captured(&first, &fixture.project.root);
    let b_file = dependency_b.project.root.join("src/lib.rs");
    assert!(!scope_a.contains(&b_file), "包外 B 尚未进入首次监听拓扑");

    // 精确重现 A 尚未登记监听时增加 B：没有通知代次帮忙，二扫却已把 B 纳入真实来源。
    fs::write(&dependency_a.project.manifest, format!(
        "[package]\nname='owned_sdk_a'\nversion='0.1.0'\n[dependencies]\nb={{package='owned_sdk_b',path='{}'}}\n",
        dependency_b.project.root.display())).unwrap();
    let second = source::capture(&fixture.project, &target, &control).unwrap();
    assert_ne!(first.sha256, second.sha256);
    let installed_b = normalized_watches(&second.watches, &fixture.project.root);
    let scope_b = EventScope::captured(&second, &fixture.project.root);
    assert_ne!(installed_a, installed_b);
    assert!(scope_b.contains(&b_file));

    for start in [WatchStart::BaselineOnly { generation: 1 }, WatchStart::RebuildCurrentSource { generation: 1 }] {
        let shared = shared();
        let (events, receiver) = mpsc::sync_channel(16);
        let mut progress = WatchProgress::new(start);
        assert!(!progress.adopt(&fixture.project.root, &installed_a, &scope_a,
            1, second.clone(), &shared, &events).unwrap());
        assert_eq!(progress.captured_generation, 0);
        assert!(!progress.baseline);
        assert!(shared.latest.lock().unwrap().is_none());
        assert!(receiver.try_recv().is_err(), "未覆盖的二扫不能发出构建等待回执");

        // 下一轮先补装 B 的真实原生监听，再复扫；只有此完整结果才有基线/排队权限。
        let mut watcher = notify::recommended_watcher(|_: notify::Result<notify::Event>| {}).unwrap();
        for (path, recursive) in &installed_b {
            watcher.watch(path, if *recursive { RecursiveMode::Recursive } else { RecursiveMode::NonRecursive }).unwrap();
        }
        let current = source::capture(&fixture.project, &target, &control).unwrap();
        assert!(progress.adopt(&fixture.project.root, &installed_b, &scope_b,
            1, current.clone(), &shared, &events).unwrap());
        assert_eq!(progress.captured_generation, 1);
        assert!(progress.baseline);
        if start.rebuild_current_source() {
            assert_eq!(shared.latest.lock().unwrap().as_ref().unwrap().source, current);
            assert!(matches!(receiver.try_recv().unwrap(), Event::Waiting { generation: 1, backend_revision: 0 }));
        } else {
            assert!(shared.latest.lock().unwrap().is_none());
            assert!(receiver.try_recv().is_err());
        }
    }
}

#[test]
fn unchanged_watch_paths_still_require_the_new_explicit_source_scope() {
    let mut fixture = Fixture::new();
    let project_root = fixture.directory.join("nested-project");
    fs::create_dir(&project_root).unwrap();
    for name in ["src", "assets", "Cargo.toml", "editor-project.json"] {
        fs::rename(fixture.directory.join(name), project_root.join(name)).unwrap();
    }
    fixture.project = crate::project::open(&project_root).unwrap();
    let script = fixture.directory.join("shared_build.rs");
    fs::write(&script, "fn main() {}\n").unwrap();
    let shared = shared();
    let target = fixture.project.root.join("out-cache");
    let first = source::capture(&fixture.project, &target, &shared.control).unwrap();
    let installed = normalized_watches(&first.watches, &fixture.project.root);
    let scope_a = EventScope::captured(&first, &fixture.project.root);
    assert!(!scope_a.contains(&script), "祖先配置监听不能冒充包外源码职责");
    fs::write(&fixture.project.manifest,
        "[package]\nname='owned_watch_fixture'\nversion='0.1.0'\nbuild='../shared_build.rs'\n").unwrap();
    let second = source::capture(&fixture.project, &target, &shared.control).unwrap();
    assert_eq!(installed, normalized_watches(&second.watches, &fixture.project.root));
    assert_ne!(first.source_watches, second.source_watches);
    let scope_b = EventScope::captured(&second, &fixture.project.root);
    assert!(scope_b.contains(&script));
    let (events, receiver) = mpsc::sync_channel(16);
    let mut progress = WatchProgress::new(WatchStart::RebuildCurrentSource { generation: 1 });
    assert!(!progress.adopt(&fixture.project.root, &installed, &scope_a,
        1, second, &shared, &events).unwrap());
    assert_eq!(progress.captured_generation, 0);
    assert!(shared.latest.lock().unwrap().is_none());
    assert!(receiver.try_recv().is_err());
    let current = source::capture(&fixture.project, &target, &shared.control).unwrap();
    assert!(progress.adopt(&fixture.project.root, &installed, &scope_b,
        1, current.clone(), &shared, &events).unwrap());
    assert_eq!(shared.latest.lock().unwrap().as_ref().unwrap().source, current);
}

#[test]
fn converged_capture_does_not_restore_a_cancelled_or_superseded_generation() {
    let fixture = Fixture::new();
    let shared = shared();
    let target = fixture.project.root.join("out-cache");
    let snapshot = source::capture(&fixture.project, &target, &shared.control).unwrap();
    let installed = normalized_watches(&snapshot.watches, &fixture.project.root);
    let scope = EventScope::captured(&snapshot, &fixture.project.root);
    let (events, receiver) = mpsc::sync_channel(16);
    let mut progress = WatchProgress::new(WatchStart::RebuildCurrentSource { generation: 1 });
    shared.advance();
    shared.advance(); // 来源字节相同也不能复活已经经历 A→B→A 的旧代次。
    assert!(!progress.adopt(&fixture.project.root, &installed, &scope,
        1, snapshot.clone(), &shared, &events).unwrap());
    stop(&shared);
    assert!(!progress.adopt(&fixture.project.root, &installed, &scope,
        3, snapshot, &shared, &events).unwrap());
    assert_eq!(progress.captured_generation, 0);
    assert!(!progress.baseline);
    assert!(shared.latest.lock().unwrap().is_none());
    assert!(receiver.try_recv().is_err());
}

fn write_export_marker(directory: &Path, export_id: &str) {
    fs::write(directory.join("feathrise-export.json"), serde_json::to_vec(&serde_json::json!({
        "format":1,"kind":"feathrise-export","project_id":"owned-watch",
        "export_id":export_id,"manifest":"export-manifest.json"
    })).unwrap()).unwrap();
}

#[test]
fn custom_output_scope_releases_failure_and_revokes_a_published_target_without_its_marker() {
    let fixture = Fixture::new();
    let scope = Arc::new(super::super::output_scope::OutputScope::default());
    let destination = fixture.project.root.join("custom-output");
    // 此处只验监听租约；实际目标路径／内容的准入另由导出包事务回归验证。
    let invalidations = Arc::new(AtomicU64::new(0));
    let invalidated = invalidations.clone();
    let lease = scope.register(&destination, &fixture.project.id, move || { invalidated.fetch_add(1, Ordering::AcqRel); }).unwrap();
    assert!(scope.register(&destination, &fixture.project.id, || {}).is_err(), "活动目标只占一个槽");
    assert!(scope.ignores(&destination.join("generated.rs"), true));
    assert!(!scope.ignores(&fixture.project.root.join("src/main.rs"), true));
    drop(lease);
    assert!(!scope.ignores(&destination.join("generated.rs"), true), "失败或取消必须归还精确排除");
    assert_eq!(invalidations.load(Ordering::Acquire), 1, "活动期吞过的真实源码通知不能随失败永久丢失");

    fs::create_dir(&destination).unwrap();
    write_export_marker(&destination, "published-one");
    scope.register(&destination, &fixture.project.id, || {}).unwrap().published("published-one".into());
    assert!(!scope.refresh());
    assert!(scope.ignores(&destination, true), "完成之后仍吸收延迟的目录rename通知");
    assert!(!scope.refresh());
    #[cfg(windows)]
    {
        scope.register(&fixture.project.root.join("CUSTOM-OUTPUT"), &fixture.project.id, || {}).unwrap()
            .published("published-one".into());
        assert!(!scope.refresh(), "Windows用户输入大小写不改变同一普通输出目录的身份");
    }
    fs::remove_file(destination.join("feathrise-export.json")).unwrap();
    assert!(scope.ignores(&destination.join("feathrise-export.json"), false));
    let invalidated = invalidations.clone();
    assert!(scope.register(&fixture.project.root.join("next-output"), &fixture.project.id, move || {
        invalidated.fetch_add(1, Ordering::AcqRel);
    }).is_err(), "新目标不能覆盖旧published槽尚未重验的失效通知");
    assert_eq!(invalidations.load(Ordering::Acquire), 2, "换槽前标记移除也必须撤销旧代次");
    assert!(!scope.ignores(&destination.join("real-source.rs"), true));
}

#[test]
fn native_custom_export_renames_keep_the_input_lease_and_real_source_aba_revokes_it() {
    let fixture = Fixture::new();
    let shared = shared();
    let worker_shared = shared.clone();
    let project = fixture.project.clone();
    let target = project.root.join("out-cache");
    let (events, receiver) = mpsc::sync_channel(16);
    let worker = thread::spawn(move || watch(project, target, worker_shared, events, WatchStart::BaselineOnly { generation: 1 }));
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        thread::sleep(Duration::from_millis(650));
        assert!(receiver.try_recv().is_err());
        let generation = shared.generation();
        let lease_shared = shared.clone();
        let control = BuildControl::with_input_guard(None, Some(crate::build_process::InputGuard(Arc::new(move ||
            !lease_shared.stopped() && lease_shared.generation() == generation))));
        let destination = fixture.project.root.join("custom-output");
        for (index, export_id) in ["first-export", "second-export"].into_iter().enumerate() {
            let stage = fixture.project.root.join(format!(".feathrise-export-stage-scope-{index}"));
            let backup = fixture.project.root.join(format!(".feathrise-export-backup-scope-{index}"));
            fs::create_dir(&stage).unwrap();
            write_export_marker(&stage, export_id);
            fs::write(stage.join("generated.rs"), "exported asset, never source").unwrap();
            let invalidated = shared.clone();
            let output = shared.output_scope.register(&destination, &fixture.project.id, move || {
                invalidated.advance();
                invalidated.wake.notify_all();
            }).unwrap();
            if index != 0 { fs::rename(&destination, &backup).unwrap(); }
            fs::rename(&stage, &destination).unwrap();
            output.published(export_id.into());
            // 在租约已完成后等待真实原生通知，覆盖迟到的首次发布和旧包更新rename。
            thread::sleep(Duration::from_millis(650));
            assert_eq!(shared.generation(), generation, "自选导出不能撤销自身源码租约");
            control.check().unwrap();
            assert!(shared.latest.lock().unwrap().is_none());
            assert!(receiver.try_recv().is_err());
        }
        let source = fixture.project.root.join("src/main.rs");
        for contents in ["fn main() { let changed = 1; }\n", "fn main() {}\n"] {
            let before = shared.generation();
            fs::write(&source, contents).unwrap();
            let deadline = Instant::now() + Duration::from_secs(6);
            while shared.generation() == before {
                assert!(Instant::now() < deadline, "真实源码保存必须继续推进代次");
                thread::sleep(POLL);
            }
        }
        assert!(control.check().is_err(), "A→B→A不能恢复旧导出的发布权");
    }));
    stop(&shared);
    let deadline = Instant::now() + Duration::from_secs(6);
    while !worker.is_finished() && Instant::now() < deadline { thread::sleep(POLL); }
    assert!(worker.is_finished(), "自有监听未收口");
    assert!(worker.join().is_ok());
    if let Err(panic) = outcome { std::panic::resume_unwind(panic); }
}

#[test]
fn directory_modify_notifications_do_not_mask_file_or_structural_changes() {
    use notify::event::{CreateKind, DataChange, MetadataKind, ModifyKind, RemoveKind, RenameMode};
    let directory = std::path::Path::new("D:/owned/src");
    for kind in [
        EventKind::Any,
        EventKind::Modify(ModifyKind::Any),
        EventKind::Modify(ModifyKind::Data(DataChange::Content)),
        EventKind::Modify(ModifyKind::Metadata(MetadataKind::Any)),
    ] {
        assert!(
            !relevant_change(&kind, directory),
            "父目录普通通知不能触发：{kind:?}"
        );
    }
    for kind in [
        EventKind::Create(CreateKind::Folder),
        EventKind::Remove(RemoveKind::Folder),
        EventKind::Modify(ModifyKind::Name(RenameMode::From)),
        EventKind::Modify(ModifyKind::Name(RenameMode::To)),
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)),
    ] {
        assert!(
            relevant_change(&kind, directory),
            "源码目录结构变化必须触发：{kind:?}"
        );
    }
    for file in [
        "D:/owned/src/main.rs",
        "D:/owned/Cargo.toml",
        "D:/owned/.cargo/config",
        "D:/owned/rust-toolchain",
    ] {
        for kind in [
            EventKind::Modify(ModifyKind::Any),
            EventKind::Modify(ModifyKind::Data(DataChange::Content)),
        ] {
            assert!(
                relevant_change(&kind, std::path::Path::new(file)),
                "源码与配置文件修改必须触发：{file}"
            );
        }
    }
}

#[test]
fn captured_watch_scope_keeps_root_structure_and_ancestor_configuration_inputs() {
    let fixture = Fixture::new();
    let snapshot = source::capture(&fixture.project, &fixture.project.root.join("out-cache"), &BuildControl::default()).unwrap();
    let inputs = EventScope::captured(&snapshot, &fixture.project.root);
    let parent = fixture.project.root.parent().unwrap();
    for path in [fixture.project.root.clone(), fixture.project.root.join("src"), parent.to_owned(),
        parent.join("Cargo.toml"), parent.join("Cargo.lock"), parent.join("rust-toolchain.toml"),
        parent.join(".cargo"), parent.join(".cargo/config"), parent.join(".cargo/config.toml")] {
        assert!(inputs.contains(&path), "仍须处理已登记根和祖先配置：{}", path.display());
    }
    for path in [parent.join("unrelated-owned-sibling"), parent.join("unrelated.rs"), parent.join("unrelated-owned-sibling/src/main.rs")] {
        assert!(!inputs.contains(&path), "祖先监听不得接受兄弟工程内容：{}", path.display());
    }

}

#[test]
fn ancestor_event_scope_keeps_config_and_package_source_without_sibling_directories() {
    let fixture = Fixture::new();
    let sdk = Fixture::new();
    let ancestor = fixture.project.root.parent().unwrap();
    let watches = BTreeSet::from([
        (fixture.project.root.clone(), true), (ancestor.to_owned(), false),
        (ancestor.join(".cargo"), false), (sdk.project.root.clone(), false),
        (sdk.project.root.join("src"), true),
    ]);
    let scope = EventScope::from_watches(&watches);
    for path in [
        ancestor.join("Cargo.toml"), ancestor.join("Cargo.lock"),
        ancestor.join("rust-toolchain.toml"), ancestor.join(".cargo"),
        ancestor.join(".cargo/config"), ancestor.join(".cargo/config.toml"),
        sdk.project.root.join("build.rs"), sdk.project.root.join("tests"),
        sdk.project.root.join("examples"), sdk.project.root.join("benches"),
        sdk.project.root.join("src/new-module"), sdk.project.root.join("src/new-module/mod.rs"),
        fixture.project.root.join("src/first-save.rs"),
    ] {
        assert!(scope.contains(&path), "真实配置、源码入口或递归源码变化不能被过滤：{}", path.display());
    }
    for path in [ancestor.join("other-owned-fixture"), ancestor.join("unrelated.rs"),
        ancestor.join("other-owned-fixture/src/main.rs")] {
        assert!(!scope.contains(&path), "祖先登记不能扩大为兄弟工程监听：{}", path.display());
    }
    let without_src = watches.iter().filter(|(path, _)| *path != sdk.project.root.join("src")).cloned().collect();
    let missing_source = EventScope::from_watches(&without_src);
    assert!(missing_source.contains(&sdk.project.root.join("src")), "包首次新建src目录也须触发复扫并登记递归监听");
    let initial = EventScope::initial(&fixture.project.root);
    assert!(initial.contains(&fixture.project.root.join("src/first-save.rs")), "首扫之前的真实工程保存也必须推进代次");
}

#[test]
fn real_native_saves_keep_only_latest_and_aba_advances_generation_without_cargo() {
    let fixture = Fixture::new();
    let shared = shared();
    let worker_shared = shared.clone();
    let project = fixture.project.clone();
    let target = project.root.join("out-cache");
    let worker_target = target.clone();
    let (events, receiver) = mpsc::sync_channel(16);
    let worker = thread::spawn(move || watch(project, worker_target, worker_shared, events, WatchStart::BaselineOnly { generation: 1 }));
    let outcome = (|| -> Result<(), String> {
        // 小型自有工程完成基线后仍不得排入首次构建。
        thread::sleep(Duration::from_millis(650));
        for event in receiver.try_iter() {
            if let Event::Failed(error) = event {
                return Err(error);
            } else {
                return Err(format!("仅打开工程却排入构建：{event:?}"));
            }
        }
        let source_file = fixture.project.root.join("src/main.rs");
        fs::write(&source_file, "fn main() { let value = 2; }\n")
            .map_err(|error| error.to_string())?;
        let deadline = Instant::now() + Duration::from_secs(6);
        let first_generation = loop {
            if let Some(generation) = shared
                .latest
                .lock()
                .unwrap()
                .as_ref()
                .map(|request| request.generation)
            {
                break generation;
            }
            if Instant::now() > deadline {
                return Err("真实保存没有进入最新等待槽".into());
            }
            thread::sleep(POLL);
        };
        fs::write(&source_file, "fn main() { let value = 3; }\n")
            .map_err(|error| error.to_string())?;
        fs::write(&source_file, "fn main() { let value = 2; }\n")
            .map_err(|error| error.to_string())?;
        let deadline = Instant::now() + Duration::from_secs(6);
        loop {
            if let Some(generation) = shared
                .latest
                .lock()
                .unwrap()
                .as_ref()
                .map(|request| request.generation)
            {
                if generation > first_generation && generation == shared.generation() {
                    break;
                }
            }
            if Instant::now() > deadline {
                return Err("改后还原未推进独立事件代次或未合并最后版本".into());
            }
            thread::sleep(POLL);
        }
        let current = source::capture(&fixture.project, &target, &shared.control)?;
        assert_eq!(
            shared.latest.lock().unwrap().as_ref().unwrap().source,
            current
        );
        assert!(!shared.current(
            first_generation,
            feathrise_editor_core::ProjectBuildProfile::Debug
        ));
        Ok(())
    })();
    stop(&shared);
    let deadline = Instant::now() + Duration::from_secs(6);
    while !worker.is_finished() && Instant::now() < deadline {
        thread::sleep(POLL);
    }
    assert!(worker.is_finished(), "源码监听取消后未确认收口");
    assert!(worker.join().is_ok());
    outcome.unwrap();
}

#[test]
fn native_watch_start_failure_revokes_old_candidate_and_stops_both_slots() {
    let fixture = Fixture::new();
    let mut project = fixture.project.clone();
    project.root = project.root.join("missing-owned-directory");
    let shared = shared();
    let (events, receiver) = mpsc::sync_channel(16);
    let target = project.root.join("out-cache");
    watch(project, target, shared.clone(), events, WatchStart::BaselineOnly { generation: 1 });
    assert!(matches!(receiver.try_recv().unwrap(), Event::Failed(_)));
    assert!(shared.stopped());
    assert!(!shared.current(1, feathrise_editor_core::ProjectBuildProfile::Debug));
}

#[test]
fn native_generated_rust_in_custom_cache_asset_and_author_roots_never_requeues_build() {
    let mut fixture = Fixture::new();
    fs::write(fixture.project.root.join("editor-project.json"), r#"{"format":1,"id":"owned-watch","name":"监听夹具","cargo_manifest":"Cargo.toml","package":"owned_watch_fixture","asset_roots":["src/player-data"],"editor_data":"src/author-store"}"#).unwrap();
    fixture.project = crate::project::open(&fixture.directory).unwrap();
    let target = fixture.project.root.join("src/out-cache");
    let shared = shared();
    let worker_shared = shared.clone();
    let project = fixture.project.clone();
    let worker_target = target.clone();
    let (events, receiver) = mpsc::sync_channel(16);
    let worker = thread::spawn(move || watch(project, worker_target, worker_shared, events, WatchStart::BaselineOnly { generation: 1 }));
    // 强断言失败也先停止真实原生监听，再恢复 panic；不能让测试失败遗留后台线程。
    let outcome =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<(), String> {
            thread::sleep(Duration::from_millis(650));
            for event in receiver.try_iter() {
                return Err(format!("首次基线出现意外事件：{event:?}"));
            }
            let generation = shared.generation();
            let author_root = fixture.project.context.editor_data.parent().unwrap();
            for directory in [
                &target,
                &fixture.project.context.asset_roots[0],
                author_root,
            ] {
                fs::create_dir_all(directory).map_err(|error| error.to_string())?;
                for name in ["generated.rs", "compiled.wgsl", "included.json"] {
                    fs::write(directory.join(name), "generated output")
                        .map_err(|error| error.to_string())?;
                }
            }
            thread::sleep(Duration::from_millis(650));
            assert_eq!(
                shared.generation(),
                generation,
                "排除目录不能推进原子源码代次"
            );
            assert!(
                shared.latest.lock().unwrap().is_none(),
                "生成 .rs 不得自触发下一轮 Cargo"
            );
            assert!(receiver.try_recv().is_err());
            Ok(())
        }));
    stop(&shared);
    let deadline = Instant::now() + Duration::from_secs(6);
    while !worker.is_finished() && Instant::now() < deadline {
        thread::sleep(POLL);
    }
    assert!(worker.is_finished(), "自有监听未收口");
    assert!(worker.join().is_ok());
    match outcome {
        Ok(result) => result.unwrap(),
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

#[test]
fn native_watch_ignores_sibling_projects_without_losing_owned_source_changes() {
    let fixture = Fixture::new();
    let shared = shared();
    let worker_shared = shared.clone();
    let project = fixture.project.clone();
    let target = project.root.join("out-cache");
    let (events, receiver) = mpsc::sync_channel(16);
    let worker = thread::spawn(move || watch(project, target, worker_shared, events, WatchStart::BaselineOnly { generation: 1 }));
    // 同父目录兄弟工程的建立和回收会命中祖先非递归监听，但不是本工程的源码输入。
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        thread::sleep(Duration::from_millis(650));
        assert!(receiver.try_recv().is_err());
        let generation = shared.generation();
        let sibling = Fixture::new();
        fs::write(sibling.project.root.join("src/main.rs"), "fn main() { let sibling = 1; }\n").unwrap();
        drop(sibling);
        thread::sleep(Duration::from_millis(650));
        assert_eq!(shared.generation(), generation, "兄弟工程的结构事件不能撤销本工程版本");
        assert!(shared.latest.lock().unwrap().is_none());
        assert!(receiver.try_recv().is_err());

        fs::write(fixture.project.root.join("src/main.rs"), "fn main() { let owned = 2; }\n").unwrap();
        let deadline = Instant::now() + Duration::from_secs(6);
        loop {
            if shared.latest.lock().unwrap().as_ref().is_some_and(|request| request.generation > generation) {
                break;
            }
            assert!(Instant::now() < deadline, "归属过滤不能漏掉本工程真实源码保存");
            thread::sleep(POLL);
        }
    }));
    stop(&shared);
    let deadline = Instant::now() + Duration::from_secs(6);
    while !worker.is_finished() && Instant::now() < deadline {
        thread::sleep(POLL);
    }
    assert!(worker.is_finished(), "自有监听未收口");
    assert!(worker.join().is_ok());
    if let Err(panic) = outcome { std::panic::resume_unwind(panic); }
}

#[test]
fn external_explicit_build_script_saves_and_aba_revoke_old_generations() {
    let mut fixture = Fixture::new();
    let project_root = fixture.directory.join("nested-project");
    fs::create_dir(&project_root).unwrap();
    for name in ["src", "assets", "Cargo.toml", "editor-project.json"] {
        fs::rename(fixture.directory.join(name), project_root.join(name)).unwrap();
    }
    fs::write(project_root.join("Cargo.toml"),
        "[package]\nname='owned_watch_fixture'\nversion='0.1.0'\nbuild='../shared_build.rs'\n").unwrap();
    let script = fixture.directory.join("shared_build.rs");
    fs::write(&script, "fn main() {}\n").unwrap();
    fixture.project = crate::project::open(&project_root).unwrap();
    let shared = shared();
    let worker_shared = shared.clone();
    let project = fixture.project.clone();
    let target = project.root.join("out-cache");
    let (events, receiver) = mpsc::sync_channel(16);
    let worker = thread::spawn(move || watch(project, target, worker_shared, events, WatchStart::BaselineOnly { generation: 1 }));
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        thread::sleep(Duration::from_millis(650));
        assert!(receiver.try_recv().is_err());
        let generation = shared.generation();
        fs::write(&script, "fn main() { let version = 2; }\n").unwrap();
        let deadline = Instant::now() + Duration::from_secs(6);
        let first_generation = loop {
            if let Some(generation) = shared.latest.lock().unwrap().as_ref().map(|request| request.generation) {
                break generation;
            }
            assert!(Instant::now() < deadline, "包外显式build.rs保存没有进入等待槽");
            thread::sleep(POLL);
        };
        assert!(first_generation > generation);
        // A→B→A 即使最终字节恢复，也必须让旧产物失效；不能只比较摘要后吞掉保存事件。
        fs::write(&script, "fn main() { let version = 3; }\n").unwrap();
        fs::write(&script, "fn main() { let version = 2; }\n").unwrap();
        let deadline = Instant::now() + Duration::from_secs(6);
        loop {
            if shared.latest.lock().unwrap().as_ref().is_some_and(|request| request.generation > first_generation) { break; }
            assert!(Instant::now() < deadline, "包外显式build.rs改后还原没有撤销旧版本");
            thread::sleep(POLL);
        }
        assert!(!shared.current(first_generation, feathrise_editor_core::ProjectBuildProfile::Debug));
    }));
    stop(&shared);
    let deadline = Instant::now() + Duration::from_secs(6);
    while !worker.is_finished() && Instant::now() < deadline { thread::sleep(POLL); }
    assert!(worker.is_finished(), "包外构建脚本监听未收口");
    assert!(worker.join().is_ok());
    if let Err(panic) = outcome { std::panic::resume_unwind(panic); }

}

/// 使用真实原生事件，但不启动构建消费者，等待槽本身就是可核对的完整调度回执。
fn wait_request(
    shared: &Shared,
    receiver: &mpsc::Receiver<Event>,
    minimum_generation: u64,
) -> Result<(u64, source::SourceSnapshot), String> {
    let deadline = Instant::now() + Duration::from_secs(6);
    loop {
        for event in receiver.try_iter() {
            if let Event::Failed(error) = event { return Err(error); }
        }
        if let Some(request) = shared.latest.lock().unwrap().as_ref()
            && request.generation >= minimum_generation
            && request.generation == shared.generation()
        {
            return Ok((request.generation, request.source.clone()));
        }
        if Instant::now() >= deadline { return Err("真实监听没有发布当前代次的完整等待请求".into()); }
        thread::sleep(POLL);
    }
}

fn finish_watch(shared: &Shared, worker: thread::JoinHandle<()>) {
    stop(shared);
    let deadline = Instant::now() + Duration::from_secs(6);
    while !worker.is_finished() && Instant::now() < deadline { thread::sleep(POLL); }
    assert!(worker.is_finished(), "恢复夹具的原生监听未确认退出");
    worker.join().unwrap();
}

#[test]
fn native_embedded_shader_and_include_str_inputs_queue_saves_create_rename_and_remove() {
    let fixture = Fixture::new();
    let shader = fixture.project.root.join("src/shaders/runtime.wgsl");
    let config = fixture.project.root.join("src/config.json");
    fs::create_dir(shader.parent().unwrap()).unwrap();
    fs::write(&shader, "const SCALE: f32 = 1.0;\n").unwrap();
    fs::write(&config, "{\"speed\":1}\n").unwrap();
    fs::write(fixture.project.root.join("src/main.rs"),
        "const SHADER: &str = include_str!(\"shaders/runtime.wgsl\");\nconst CONFIG: &str = include_str!(\"config.json\");\nfn main() {}\n").unwrap();
    let shared = shared();
    let thread_shared = shared.clone();
    let project = fixture.project.clone();
    let target = project.root.join("out-cache");
    let thread_target = target.clone();
    let (events, receiver) = mpsc::sync_channel(16);
    let worker = thread::spawn(move || watch(project, thread_target, thread_shared, events,
        WatchStart::RebuildCurrentSource { generation: 1 }));
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<(), String> {
        let (mut generation, initial) = wait_request(&shared, &receiver, 1)?;
        for (path, text) in [(&shader, "const SCALE: f32 = 2.0;\n"), (&config, "{\"speed\":2}\n")] {
            fs::write(path, text).map_err(|error| error.to_string())?;
            let (next, snapshot) = wait_request(&shared, &receiver, generation + 1)?;
            assert_ne!(snapshot.sha256, initial.sha256, "非Rust文件确实进入原完整源码摘要");
            assert_eq!(snapshot, source::capture(&fixture.project, &target, &shared.control)?);
            generation = next;
        }
        // 未枚举的扩展名从已扫描源码树的新建事件进入；不靠WGSL/JSON白名单。
        let extra = fixture.project.root.join("src/shaders/input.custom-input");
        fs::write(&extra, "first input\n").map_err(|error| error.to_string())?;
        generation = wait_request(&shared, &receiver, generation + 1)?.0;
        fs::write(&extra, "changed input\n").map_err(|error| error.to_string())?;
        generation = wait_request(&shared, &receiver, generation + 1)?.0;
        let renamed = extra.with_extension("renamed-input");
        fs::rename(&extra, &renamed).map_err(|error| error.to_string())?;
        generation = wait_request(&shared, &receiver, generation + 1)?.0;
        fs::remove_file(&renamed).map_err(|error| error.to_string())?;
        wait_request(&shared, &receiver, generation + 1)?;

        let stable = shared.generation();
        shared.latest.lock().unwrap().take();
        // 工程根的无关JSON不在捕获范围；祖先非递归监听也不能把兄弟文件扩成编译输入。
        fs::write(fixture.project.root.join("notes.json"), "{\"unrelated\":true}")
            .map_err(|error| error.to_string())?;
        thread::sleep(Duration::from_millis(650));
        assert_eq!(shared.generation(), stable);
        assert!(shared.latest.lock().unwrap().is_none());
        Ok(())
    }));
    finish_watch(&shared, worker);
    match outcome { Ok(result) => result.unwrap(), Err(panic) => std::panic::resume_unwind(panic) }
}

fn wait_capture_failure(shared: &Shared, receiver: &mpsc::Receiver<Event>, minimum_generation: u64) {
    let deadline = Instant::now() + Duration::from_secs(6);
    loop {
        for event in receiver.try_iter() {
            if let Event::Failed(error) = event {
                assert!(!error.is_empty());
                assert!(shared.generation() >= minimum_generation);
                assert!(shared.latest.lock().unwrap().is_none(), "缺少显式构建输入不能发成功构建请求");
                return;
            }
        }
        assert!(Instant::now() < deadline, "删除/移走显式编译输入没有推进代次并报告来源缺失");
        thread::sleep(POLL);
    }
}

#[test]
fn native_external_build_script_rename_and_delete_revoke_then_restore_source_capture() {
    let mut fixture = Fixture::new();
    let project_root = fixture.directory.join("nested-project");
    fs::create_dir(&project_root).unwrap();
    for name in ["src", "assets", "Cargo.toml", "editor-project.json"] {
        fs::rename(fixture.directory.join(name), project_root.join(name)).unwrap();
    }
    fs::write(project_root.join("Cargo.toml"),
        "[package]\nname='owned_watch_fixture'\nversion='0.1.0'\nbuild='../shared_build.rs'\n").unwrap();
    let script = fixture.directory.join("shared_build.rs");
    let moved = fixture.directory.join("shared_build.saved-input");
    fs::write(&script, "fn main() {}\n").unwrap();
    fixture.project = crate::project::open(&project_root).unwrap();
    let shared = shared();
    let thread_shared = shared.clone();
    let project = fixture.project.clone();
    let target = project.root.join("out-cache");
    let (events, receiver) = mpsc::sync_channel(16);
    let worker = thread::spawn(move || watch(project, target, thread_shared, events,
        WatchStart::RebuildCurrentSource { generation: 1 }));
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let (initial, _) = wait_request(&shared, &receiver, 1).unwrap();
        shared.latest.lock().unwrap().take();
        fs::rename(&script, &moved).unwrap();
        wait_capture_failure(&shared, &receiver, initial + 1);
        assert!(!shared.current(initial, feathrise_editor_core::ProjectBuildProfile::Debug));
        let restore_after = shared.generation();
        fs::rename(&moved, &script).unwrap();
        let (restored, _) = wait_request(&shared, &receiver, restore_after + 1).unwrap();
        shared.latest.lock().unwrap().take();
        fs::remove_file(&script).unwrap();
        wait_capture_failure(&shared, &receiver, restored + 1);
        let recreate_after = shared.generation();
        fs::write(&script, "fn main() {}\n").unwrap();
        wait_request(&shared, &receiver, recreate_after + 1).unwrap();
    }));
    finish_watch(&shared, worker);
    if let Err(panic) = outcome { std::panic::resume_unwind(panic); }
}

#[test]
fn native_declared_font_asset_changes_revoke_patch_identity_without_watching_other_assets() {
    let fixture = Fixture::new();
    let font = fixture.project.root.join("assets/fonts/test.otf");
    let ordinary = fixture.project.root.join("assets/fonts/ordinary.otf");
    fs::create_dir(font.parent().unwrap()).unwrap();
    fs::write(&font, b"owned font one").unwrap();
    fs::write(&ordinary, b"ordinary runtime asset").unwrap();
    fs::write(&fixture.project.manifest,
        "[package]\nname='owned_watch_fixture'\nversion='0.1.0'\n[package.metadata.feathrise]\ncompiled-inputs=['assets/fonts/test.otf']\n[package.metadata.feathrise.hotpatch]\nenabled=true\nfunctions=['src/main.rs::speed']\n").unwrap();
    let entry = fixture.project.root.join("src/main.rs");
    fs::write(&entry,
        "const FONT: &[u8] = include_bytes!(\"../assets/fonts/test.otf\");\npub fn speed() -> f32 { 1.0 }\nfn main() {}\n").unwrap();
    let shared = shared();
    let thread_shared = shared.clone();
    let project = fixture.project.clone();
    let target = project.root.join("out-cache");
    let (events, receiver) = mpsc::sync_channel(16);
    let worker = thread::spawn(move || watch(project, target, thread_shared, events,
        WatchStart::RebuildCurrentSource { generation: 1 }));
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let (initial, baseline) = wait_request(&shared, &receiver, 1).unwrap();
        assert!(baseline.hotpatch_shape.is_some());
        let scope = EventScope::captured(&baseline, &fixture.project.root);
        let structure = EventKind::Modify(notify::event::ModifyKind::Name(notify::event::RenameMode::From));
        assert!(scope.compiled_ancestor_change(&structure, font.parent().unwrap()));
        assert!(!scope.compiled_ancestor_change(&EventKind::Modify(notify::event::ModifyKind::Any), font.parent().unwrap()));
        assert!(!scope.compiled_ancestor_change(&structure, &font.parent().unwrap().join("side-directory")));
        shared.latest.lock().unwrap().take();
        fs::write(&ordinary, b"changed ordinary runtime asset").unwrap();
        fs::write(font.parent().unwrap().join("unlisted.rs"), "fn runtime_asset() {}\n").unwrap();
        thread::sleep(Duration::from_millis(650));
        assert_eq!(shared.generation(), initial, "精确字体声明不能授权同目录其它资产，即使名为.rs");
        assert!(shared.latest.lock().unwrap().is_none());

        fs::write(&font, b"owned font two").unwrap();
        let (font_generation, changed_font) = wait_request(&shared, &receiver, initial + 1).unwrap();
        assert_ne!(baseline.sha256, changed_font.sha256);
        assert_ne!(baseline.hotpatch_shape, changed_font.hotpatch_shape);
        fs::write(&entry,
            "const FONT: &[u8] = include_bytes!(\"../assets/fonts/test.otf\");\npub fn speed() -> f32 { 2.0 }\nfn main() {}\n").unwrap();
        let (body_generation, changed_body) = wait_request(&shared, &receiver, font_generation + 1).unwrap();
        assert_eq!(changed_font.hotpatch_shape, changed_body.hotpatch_shape);
        assert_ne!(baseline.hotpatch_shape, changed_body.hotpatch_shape,
            "后续登记函数体保存仍不能把已改变字体归为原fat基线的纯函数补丁");
        shared.latest.lock().unwrap().take();
        fs::remove_file(&font).unwrap();
        wait_capture_failure(&shared, &receiver, body_generation + 1);
        let restore_after = shared.generation();
        fs::write(&font, b"owned font one").unwrap();
        let (restored_generation, _) = wait_request(&shared, &receiver, restore_after + 1).unwrap();
        shared.latest.lock().unwrap().take();
        let parent = font.parent().unwrap();
        let retired = parent.with_file_name("fonts-retired");
        fs::rename(parent, &retired).unwrap();
        wait_capture_failure(&shared, &receiver, restored_generation + 1);
        let restore_directory_after = shared.generation();
        fs::rename(&retired, parent).unwrap();
        let (directory_generation, _) = wait_request(&shared, &receiver, restore_directory_after + 1).unwrap();
        // 目录曾被移走，旧路径仍在内存表不代表notify仍持有效watch；恢复后再存必须实际收到通知。
        fs::write(&font, b"owned font three").unwrap();
        let (_, after_directory_recovery) = wait_request(&shared, &receiver, directory_generation + 1).unwrap();
        assert_ne!(baseline.hotpatch_shape, after_directory_recovery.hotpatch_shape);
        let stable = shared.generation();
        shared.latest.lock().unwrap().take();
        fs::write(&ordinary, b"ordinary asset after directory recovery").unwrap();
        thread::sleep(Duration::from_millis(650));
        assert_eq!(shared.generation(), stable, "重装监听后旁支资产仍不触发编译");
        assert!(shared.latest.lock().unwrap().is_none());
    }));
    finish_watch(&shared, worker);
    if let Err(panic) = outcome { std::panic::resume_unwind(panic); }
}

#[test]
fn resumed_native_watch_compensates_unwatched_saves_and_keeps_later_saves_latest() {
    let fixture = Fixture::new();
    let target = fixture.project.root.join("out-cache");
    let old_shared = shared_at(41);
    let old_thread_shared = old_shared.clone();
    let old_project = fixture.project.clone();
    let old_target = target.clone();
    let (old_events, old_receiver) = mpsc::sync_channel(16);
    let old_worker = thread::spawn(move || watch(old_project, old_target, old_thread_shared, old_events,
        WatchStart::RebuildCurrentSource { generation: 41 }));
    let initial = wait_request(&old_shared, &old_receiver, 41);
    // 只有旧真实监听完全退出并join之后才建立新监听，不能把 cancel 当作已经退役。
    finish_watch(&old_shared, old_worker);
    let (old_generation, initial) = initial.unwrap();
    let final_generation = old_shared.generation();
    assert_eq!(old_generation, final_generation);
    assert!(!old_shared.current(final_generation, feathrise_editor_core::ProjectBuildProfile::Debug));

    let source_file = fixture.project.root.join("src/main.rs");
    fs::write(&source_file, "fn main() { let during_close = 2; }\n").unwrap();
    fs::write(&source_file, "fn main() {}\n").unwrap(); // 停监听窗口内改后还原也要获得新生命周期代次。
    fs::write(&source_file, "fn main() { let during_close = 3; }\n").unwrap();
    assert_eq!(old_shared.generation(), final_generation, "旧回调已经完整收口");

    let resumed_generation = final_generation.checked_add(1).unwrap();
    let resumed = shared_at(resumed_generation);
    let resumed_shared = resumed.clone();
    let project = fixture.project.clone();
    let worker_target = target.clone();
    let (events, receiver) = mpsc::sync_channel(16);
    let worker = thread::spawn(move || watch(project, worker_target, resumed_shared, events,
        WatchStart::RebuildCurrentSource { generation: resumed_generation }));
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<(), String> {
        let (compensated_generation, compensated) = wait_request(&resumed, &receiver, resumed_generation)?;
        assert!(compensated_generation > final_generation);
        assert_ne!(compensated.sha256, initial.sha256);
        assert_eq!(compensated, source::capture(&fixture.project, &target, &resumed.control)?);
        assert!(!resumed.current(final_generation, feathrise_editor_core::ProjectBuildProfile::Debug));

        fs::write(&source_file, "fn main() { let after_resume = 4; }\n").map_err(|error| error.to_string())?;
        let (saved_generation, latest) = wait_request(&resumed, &receiver, compensated_generation + 1)?;
        assert!(saved_generation > compensated_generation);
        assert_eq!(latest, source::capture(&fixture.project, &target, &resumed.control)?);
        assert_ne!(latest.sha256, compensated.sha256);
        Ok(())
    }));
    finish_watch(&resumed, worker);
    match outcome { Ok(result) => result.unwrap(), Err(panic) => std::panic::resume_unwind(panic) }
}

#[test]
fn resumed_native_watch_rebuilds_even_when_unwatched_aba_restores_identical_bytes() {
    let fixture = Fixture::new();
    let target = fixture.project.root.join("out-cache");
    let initial = source::capture(&fixture.project, &target, &BuildControl::default()).unwrap();
    let source_file = fixture.project.root.join("src/main.rs");
    fs::write(&source_file, "fn main() { let unwatched = 5; }\n").unwrap();
    fs::write(&source_file, "fn main() {}\n").unwrap();
    let shared = shared_at(82);
    let worker_shared = shared.clone();
    let project = fixture.project.clone();
    let worker_target = target.clone();
    let (events, receiver) = mpsc::sync_channel(16);
    let worker = thread::spawn(move || watch(project, worker_target, worker_shared, events,
        WatchStart::RebuildCurrentSource { generation: 82 }));
    let outcome = wait_request(&shared, &receiver, 82);
    finish_watch(&shared, worker);
    let (generation, request) = outcome.unwrap();
    assert_eq!(generation, 82);
    assert_eq!(request, initial, "源码字节相同仍必须补发恢复构建，不能被当成首次打开基线");
}

#[test]
fn real_native_ancestor_watch_ignores_sibling_fixture_creation_and_removal() {
    let fixture = Fixture::new();
    let target = fixture.project.root.join("out-cache");
    let shared = shared_at(91);
    let worker_shared = shared.clone();
    let project = fixture.project.clone();
    let worker_target = target.clone();
    let (events, receiver) = mpsc::sync_channel(16);
    let worker = thread::spawn(move || watch(project, worker_target, worker_shared, events,
        WatchStart::RebuildCurrentSource { generation: 91 }));
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<(), String> {
        let (generation, _) = wait_request(&shared, &receiver, 91)?;
        // 与并行测试使用相同父目录，真实创建/删除三个兄弟工程复现原来的代次+3干扰。
        for _ in 0..3 { drop(Fixture::new()); }
        thread::sleep(Duration::from_millis(650));
        assert_eq!(shared.generation(), generation, "无关兄弟目录不得污染本工程保存代次");
        fs::write(fixture.project.root.join("src/main.rs"), "fn main() { let own_save = 6; }\n")
            .map_err(|error| error.to_string())?;
        let (saved_generation, latest) = wait_request(&shared, &receiver, generation + 1)?;
        assert!(saved_generation > generation, "过滤无关目录后真实工程保存仍必须生效");
        assert_eq!(latest, source::capture(&fixture.project, &target, &shared.control)?);
        Ok(())
    }));
    finish_watch(&shared, worker);
    match outcome { Ok(result) => result.unwrap(), Err(panic) => std::panic::resume_unwind(panic) }
}
