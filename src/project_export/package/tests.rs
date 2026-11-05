//! 导出事务确定性回归；运行文件使用字节夹具，仅验证文件事务，不冒充真实程序启动验收。

use super::*;
use crate::build_artifacts::{self, PublishedBundle, RuntimeFile};
use std::{fs, path::PathBuf, sync::mpsc};

struct Fixture { root: PathBuf, project: Project, target: PathBuf, bundle: PublishedBundle }

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("feathrise-export-package-tests-{}", bevy::asset::uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let project_root = root.join("中文 工程");
        fs::create_dir_all(project_root.join("assets/empty")).unwrap();
        fs::create_dir_all(project_root.join("secondary/models")).unwrap();
        fs::create_dir(project_root.join("src")).unwrap();
        fs::write(project_root.join("src/main.rs"), "作者源码，不进入导出").unwrap();
        fs::write(project_root.join("Cargo.toml"), "[package]\nname='fixture_game'\nversion='0.1.0'\n").unwrap();
        fs::write(project_root.join("editor-project.json"), serde_json::to_vec_pretty(&serde_json::json!({
            "format":2,"id":"045f04e0-1234-4abc-8def-123456789abc","name":"中文 工程","package":"fixture_game",
            "cargo_manifest":"Cargo.toml","asset_roots":["assets","secondary"],"editor_data":".editor",
            "game_entry":{"kind":"saved-world","version":1},"custom_field":{"preserved":true}
        })).unwrap()).unwrap();
        fs::write(project_root.join("scene.layout.json"), serde_json::to_vec(&serde_json::json!({
            "format":1,"project_id":"045f04e0-1234-4abc-8def-123456789abc","revision":1,"unknown":42
        })).unwrap()).unwrap();
        fs::write(project_root.join("assets/config.json"), b"{\"value\":1}").unwrap();
        fs::write(project_root.join("secondary/models/mesh.glb"), b"saved model bytes").unwrap();
        let project = crate::project::open(&project_root).unwrap();
        fs::create_dir_all(&project.context.editor_data).unwrap();
        fs::write(project.context.editor_data.join("author-private.txt"), b"private draft").unwrap();
        let target = root.join("cache");
        fs::create_dir(&target).unwrap();
        let executable = root.join("fixture_game.exe");
        fs::write(&executable, b"runtime-byte-fixture").unwrap();
        let dll = root.join("fixture-runtime.dll");
        fs::write(&dll, b"dll-byte-fixture").unwrap();
        let source_identity = crate::code_iteration::export_source_identity(&project, &target, &BuildControl::default()).unwrap();
        let bundle = build_artifacts::publish(&target, &project.context.storage_key, &executable,
            &[RuntimeFile { source:dll,relative:"fixture-runtime.dll".into() }],
            serde_json::json!({"project_id":project.id,"project_root":project.root,"profile":"render-dev","export_source_identity":source_identity}), &BuildControl::default()).unwrap();
        Self { root, project, target, bundle }
    }

    fn export(&self, destination: &Path, control: &BuildControl) -> Result<ExportReceipt, String> {
        let (sender, _) = mpsc::sync_channel(32);
        export_bundle(&self.project, &self.bundle, destination, &self.target, control, &sender)
    }

    fn output(&self) -> PathBuf { self.project.root.join("output") }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // 仅清理本测试独占创建并复核UUID名称的临时根，绝不接触真实工程或已有target。
        if let Ok(root) = paths::checked(&self.root, true) {
            let owned = root.file_name().and_then(|name| name.to_str())
                .and_then(|name| name.strip_prefix("feathrise-export-package-tests-"))
                .is_some_and(|id| bevy::asset::uuid::Uuid::parse_str(id).is_ok());
            if owned && files::tree(&root, &BuildControl::default()).is_ok() { let _ = fs::remove_dir_all(root); }
        }
    }
}

#[test]
fn default_output_contains_complete_saved_runtime_without_source_or_author_data() {
    let fixture = Fixture::new();
    let receipt = fixture.export(Path::new("output"), &BuildControl::default()).unwrap();
    assert_eq!(receipt.directory, fixture.output());
    assert_eq!(receipt.executable, fixture.output().join("fixture_game.exe"));
    assert_eq!(fs::read(receipt.executable).unwrap(), b"runtime-byte-fixture");
    assert_eq!(fs::read(fixture.output().join("fixture-runtime.dll")).unwrap(), b"dll-byte-fixture");
    for relative in ["editor-project.json", "scene.layout.json", "assets/config.json", "secondary/models/mesh.glb"] {
        assert_eq!(fs::read(fixture.output().join(relative)).unwrap(), fs::read(fixture.project.root.join(relative)).unwrap());
    }
    assert!(fixture.output().join("assets/empty").is_dir());
    for excluded in ["src", "Cargo.toml", ".editor", "target", "runtime-manifest.json", STAGE_NAME] {
        assert!(!fixture.output().join(excluded).exists(), "{excluded} must not be exported");
    }
    let verified = manifest::verify(&fixture.output(), &fixture.project, &BuildControl::default(), false).unwrap();
    assert_eq!(verified.marker.profile, "render-dev");
    assert_eq!(verified.marker.manifest, "export-manifest.json");
    let marker = fs::read_to_string(fixture.output().join(manifest::MARKER)).unwrap();
    assert!(!marker.contains("project_root") && !marker.contains("cache") && marker.len() < 64 * 1024);
    assert_eq!(receipt.files, 8);
    assert!(receipt.bytes > 0 && receipt.warning.is_none());
}

const STAGE_NAME: &str = ".feathrise-export-stage-owner";

#[test]
fn independently_moved_package_keeps_exact_relative_identity_and_hashes() {
    let fixture = Fixture::new();
    fixture.export(&fixture.output(), &BuildControl::default()).unwrap();
    let moved = fixture.root.join("可移动 独立包");
    fs::rename(fixture.output(), &moved).unwrap();
    let verified = manifest::verify(&moved, &fixture.project, &BuildControl::default(), false).unwrap();
    assert_eq!(verified.marker.executable, PathBuf::from("fixture_game.exe"));
    assert_eq!(fs::read(moved.join("secondary/models/mesh.glb")).unwrap(), b"saved model bytes");
}

#[cfg(windows)]
#[test]
fn long_chinese_asset_and_destination_names_can_be_exported() {
    let fixture = Fixture::new();
    let name = format!("{}.glb", "中".repeat(100));
    let asset = fixture.project.root.join("assets").join(&name);
    fs::write(&asset, b"valid Windows asset").unwrap();
    let destination = fixture.root.join("出".repeat(100));
    let receipt = fixture.export(&destination, &BuildControl::default()).unwrap();
    assert_eq!(receipt.directory, destination);
    assert_eq!(fs::read(receipt.directory.join("assets").join(name)).unwrap(), b"valid Windows asset");
    manifest::verify(&receipt.directory, &fixture.project, &BuildControl::default(), false).unwrap();
}

#[test]
fn repeated_export_replaces_only_verified_previous_package() {
    let fixture = Fixture::new();
    fixture.export(&fixture.output(), &BuildControl::default()).unwrap();
    let first = fs::read(fixture.output().join(manifest::MARKER)).unwrap();
    fs::write(fixture.project.root.join("assets/config.json"), b"{\"value\":2}").unwrap();
    let receipt = fixture.export(&fixture.output(), &BuildControl::default()).unwrap();
    assert!(receipt.warning.is_none());
    assert_ne!(first, fs::read(fixture.output().join(manifest::MARKER)).unwrap());
    assert_eq!(fs::read(fixture.output().join("assets/config.json")).unwrap(), b"{\"value\":2}");
    assert!(!fs::read_dir(&fixture.project.root).unwrap().any(|entry| entry.unwrap().file_name().to_string_lossy().starts_with(".feathrise-export-")));
}

#[test]
fn unowned_nonempty_directory_is_never_overwritten() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.output()).unwrap();
    fs::write(fixture.output().join("important.txt"), b"author bytes").unwrap();
    assert!(fixture.export(&fixture.output(), &BuildControl::default()).is_err());
    assert_eq!(fs::read(fixture.output().join("important.txt")).unwrap(), b"author bytes");
    assert!(!fixture.output().join("fixture_game.exe").exists());
}

#[test]
fn custom_target_listener_is_registered_only_after_path_and_existing_content_validation() {
    use std::sync::{Arc, Mutex};
    let fixture = Fixture::new();
    let registrations = Arc::new(Mutex::new(Vec::new()));
    let completions = Arc::new(Mutex::new(Vec::new()));
    let observed = registrations.clone();
    let completed = completions.clone();
    let control = BuildControl::with_export_guards(None, None, Some(crate::build_process::OutputRegistrar(Arc::new(move |path| {
        observed.lock().unwrap().push(path.to_owned());
        let completed = completed.clone();
        Ok(crate::build_process::OutputLease::new(move |published| completed.lock().unwrap().push(published)))
    }))));
    let destination = fixture.project.root.join("custom-output");
    assert!(fixture.export(&fixture.project.root.join("src"), &control).is_err());
    fs::create_dir(&destination).unwrap();
    fs::write(destination.join("author.txt"), b"unowned content").unwrap();
    assert!(fixture.export(&destination, &control).is_err());
    assert!(registrations.lock().unwrap().is_empty(), "无归属目标不得先吞掉源码事件");
    fs::remove_file(destination.join("author.txt")).unwrap();
    fixture.export(&destination, &control).unwrap();
    fixture.export(&destination, &control).unwrap();
    assert_eq!(*registrations.lock().unwrap(), vec![destination.clone(), destination]);
    let completions = completions.lock().unwrap();
    assert_eq!(completions.len(), 2);
    assert!(completions.iter().all(|id| id.as_deref().is_some_and(|id| bevy::asset::uuid::Uuid::parse_str(id).is_ok())));
}

#[test]
fn changed_previous_file_and_extra_empty_directory_block_replacement() {
    let fixture = Fixture::new();
    fixture.export(&fixture.output(), &BuildControl::default()).unwrap();
    fs::write(fixture.output().join("assets/config.json"), b"external modification").unwrap();
    assert!(fixture.export(&fixture.output(), &BuildControl::default()).is_err());
    assert_eq!(fs::read(fixture.output().join("assets/config.json")).unwrap(), b"external modification");
    fs::write(fixture.output().join("assets/config.json"), b"{\"value\":1}").unwrap();
    fs::create_dir(fixture.output().join("author folder")).unwrap();
    assert!(fixture.export(&fixture.output(), &BuildControl::default()).is_err());
    assert!(fixture.output().join("author folder").is_dir());
}

#[test]
fn output_cannot_overlap_source_assets_author_data_cache_or_engine_source() {
    let fixture = Fixture::new();
    let control = BuildControl::default();
    for destination in [fixture.project.root.clone(), fixture.root.clone(), fixture.project.root.join("assets/output"),
        fixture.project.root.join(".editor/output"), fixture.target.join("output"), fixture.project.root.join("src")] {
        assert!(fixture.export(&destination, &control).is_err(), "{}", destination.display());
    }
    // Cargo 的测试、示例与基准目录同样承担源码职责，目录暂空也不能变成发布目标。
    for source in ["tests", "examples", "benches"] {
        let directory = fixture.project.root.join(source);
        fs::create_dir(&directory).unwrap();
        let error = fixture.export(&directory.join("custom-output"), &control).unwrap_err();
        assert!(error.contains("工程源码"), "{error}");
        assert!(fs::read_dir(directory).unwrap().next().is_none());
    }
    assert_eq!(fs::read(fixture.project.root.join("src/main.rs")).unwrap(), "作者源码，不进入导出".as_bytes());
}

#[cfg(windows)]
#[test]
fn windows_directory_comparison_is_symmetric_and_preserves_component_boundaries() {
    let assets = Path::new(r"C:\project\assets");
    let output = Path::new(r"C:\project\output");
    assert!(!paths::contains(assets, output));
    assert!(!paths::contains(output, assets), "排序大于不能误判为相同目录");

    let upper = Path::new(r"C:\project\Ässets");
    let lower = Path::new(r"C:\project\ässets");
    assert!(paths::contains(upper, lower));
    assert!(paths::contains(lower, upper));
    let child = Path::new(r"C:\project\ässets\nested\output");
    assert!(paths::contains(upper, child));
    assert!(!paths::contains(child, upper));
    assert!(!paths::contains(upper, Path::new(r"C:\project\ässets-extra\output")));
}

#[cfg(windows)]
#[test]
fn unicode_case_aliases_cannot_repurpose_an_empty_asset_root_as_export_output() {
    let mut fixture = Fixture::new();
    let assets = fixture.project.root.join("Ässets");
    fs::create_dir(&assets).unwrap();
    let descriptor = fixture.project.root.join("editor-project.json");
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&descriptor).unwrap()).unwrap();
    value["asset_roots"].as_array_mut().unwrap().push("Ässets".into());
    let saved_descriptor = serde_json::to_vec_pretty(&value).unwrap();
    fs::write(&descriptor, &saved_descriptor).unwrap();
    fixture.project = crate::project::open(&fixture.project.root).unwrap();
    let alias = fixture.project.root.join("ässets");
    assert_eq!(assets.canonicalize().unwrap(), alias.canonicalize().unwrap(), "本机大小写变体必须指向同一实际目录");
    // 既保护被直接选中的空资产根，也保护输入中既有父目录使用大小写别名的子目标。
    for destination in [&alias, &alias.join("nested-output")] {
        let error = paths::destination(&fixture.project, destination, &fixture.target).unwrap_err();
        assert!(error.contains("资产根"), "{error}");
    }
    assert!(fs::read_dir(&assets).unwrap().next().is_none(), "拒绝后空资产根必须保持原状");
    assert_eq!(fs::read(descriptor).unwrap(), saved_descriptor);
    assert_eq!(fs::read(fixture.project.root.join("assets/config.json")).unwrap(), b"{\"value\":1}");
}

#[test]
fn cancellation_preserves_old_package_and_reclaims_only_owned_stage() {
    let fixture = Fixture::new();
    fixture.export(&fixture.output(), &BuildControl::default()).unwrap();
    let before = fs::read(fixture.output().join(manifest::MARKER)).unwrap();
    let control = BuildControl::default();
    let stage_path;
    {
        let stage = transaction::Stage::create(&fixture.output()).unwrap();
        stage_path = stage.path.clone();
        fs::write(stage.path.join("partial.bin"), b"unfinished bytes").unwrap();
        control.cancel();
        assert!(fixture.export(&fixture.output(), &control).is_err());
    }
    assert!(!stage_path.exists());
    assert_eq!(fs::read(fixture.output().join(manifest::MARKER)).unwrap(), before);
}

#[test]
fn saved_file_changes_and_additions_invalidate_source_snapshot() {
    let fixture = Fixture::new();
    let control = BuildControl::default();
    let plan = files::Plan::collect(&fixture.project, &fixture.bundle, &control).unwrap();
    fs::write(fixture.project.root.join("scene.layout.json"), serde_json::to_vec(&serde_json::json!({
        "format":1,"project_id":fixture.project.id,"revision":2,"unknown":42,"changed":true
    })).unwrap()).unwrap();
    assert!(plan.verify_sources(&fixture.project, &fixture.bundle, &fixture.target, &control).is_err());
    let plan = files::Plan::collect(&fixture.project, &fixture.bundle, &control).unwrap();
    fs::write(fixture.project.root.join("assets/new.bin"), b"new saved file").unwrap();
    assert!(plan.verify_sources(&fixture.project, &fixture.bundle, &fixture.target, &control).is_err());
    assert!(!fixture.output().exists());
}

#[test]
fn copying_changed_length_never_writes_beyond_the_frozen_file_budget() {
    let fixture = Fixture::new();
    let control = BuildControl::default();
    let source = fixture.root.join("changing-source.bin");
    // 小字节夹具直接提供收集阶段冻结的长度，覆盖复制前或复制期间已观察到的增长／截短。
    fs::write(&source, b"growing source").unwrap();
    let grown = fixture.root.join("grown-copy.bin");
    let error = files::copy_file(&source, &grown, 4, &control).unwrap_err();
    assert!(error.contains("增长"));
    assert!(fs::metadata(&grown).unwrap().len() <= 4, "超出冻结长度的块不能先写入暂存");

    fs::write(&source, b"tiny").unwrap();
    let shortened = fixture.root.join("shortened-copy.bin");
    let error = files::copy_file(&source, &shortened, 8, &control).unwrap_err();
    assert!(error.contains("缩短"));
    assert_eq!(fs::read(shortened).unwrap(), b"tiny");
    assert!(!fixture.output().exists());
}

#[test]
fn invalid_saved_startup_identity_or_selection_preserves_previous_export() {
    let fixture = Fixture::new();
    let control = BuildControl::default();
    fixture.export(&fixture.output(), &control).unwrap();
    let original = fs::read(fixture.output().join(manifest::MARKER)).unwrap();
    for value in [
        serde_json::json!({"format":2,"project_id":fixture.project.id,"revision":1}),
        serde_json::json!({"format":1,"project_id":"another-project","revision":1}),
        serde_json::json!({"format":1,"project_id":fixture.project.id,"revision":0}),
        serde_json::json!({"format":1,"project_id":fixture.project.id,"revision":1,"production_map":null}),
        serde_json::json!({"format":1,"project_id":fixture.project.id,"revision":1,"production_map":"MAP-MISSING"}),
    ] {
        fs::write(fixture.project.root.join("scene.layout.json"), serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(fixture.export(&fixture.output(), &control).is_err(), "{value}");
        assert_eq!(fs::read(fixture.output().join(manifest::MARKER)).unwrap(), original);
    }
}

#[test]
fn empty_saved_world_without_startup_document_can_be_exported() {
    let fixture = Fixture::new();
    fs::remove_file(fixture.project.root.join("scene.layout.json")).unwrap();
    let receipt = fixture.export(&fixture.output(), &BuildControl::default()).unwrap();
    assert_eq!(receipt.files, 7);
    assert!(!fixture.output().join("scene.layout.json").exists());
    assert!(receipt.executable.is_file());
}

#[test]
fn destination_created_after_preflight_is_preserved() {
    let fixture = Fixture::new();
    let control = BuildControl::default();
    let mut stage = transaction::Stage::create(&fixture.output()).unwrap();
    fs::create_dir(fixture.output()).unwrap();
    fs::write(fixture.output().join("arrived.txt"), b"author file").unwrap();
    assert!(stage.publish(&fixture.output(), None, &fixture.project, &control).is_err());
    assert_eq!(fs::read(fixture.output().join("arrived.txt")).unwrap(), b"author file");
}

#[test]
fn relative_paths_reject_windows_devices_and_traversal() {
    for path in ["", "../escape", "/absolute", "CON", "aux.txt", "bad:stream", "trailing.", "trailing ", "COM1.log"] {
        assert!(paths::relative(Path::new(path)).is_err(), "{path}");
    }
    assert!(paths::relative(Path::new("assets/中文 模型.glb")).is_ok());
}

#[test]
fn relative_paths_measure_windows_names_in_utf16_units() {
    // 合法中文名称的 UTF-8 字节可能超过255；非BMP字符则占两个 UTF-16 单元。
    let chinese = format!("{}.glb", "中".repeat(100));
    assert!(chinese.len() > 255);
    assert!(paths::relative(Path::new(&chinese)).is_ok());
    let full_budget = format!("{}a", "😀".repeat(127));
    assert!(paths::relative(Path::new(&full_budget)).is_ok());
    assert!(paths::relative(Path::new(&format!("{full_budget}a"))).is_err());
    assert!(paths::relative(Path::new(&"a".repeat(256))).is_err());
}

#[test]
fn explicitly_selected_empty_directory_is_allowed() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.output()).unwrap();
    let receipt = fixture.export(&fixture.output(), &BuildControl::default()).unwrap();
    assert!(receipt.executable.is_file());
    assert!(receipt.warning.is_none());
}

#[test]
fn source_change_after_runtime_build_is_rejected_even_without_live_watcher() {
    let fixture = Fixture::new();
    fs::write(fixture.project.root.join("src/main.rs"), "changed after build").unwrap();
    let error = fixture.export(&fixture.output(), &BuildControl::default()).unwrap_err();
    assert!(error.contains("源码已变化"));
    assert!(!fixture.output().exists());
}

#[test]
fn nested_asset_root_preserves_all_intermediate_and_empty_directories() {
    let mut fixture = Fixture::new();
    let descriptor = fixture.project.root.join("editor-project.json");
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&descriptor).unwrap()).unwrap();
    value["asset_roots"] = serde_json::json!(["assets", "data/nested/assets"]);
    fs::create_dir_all(fixture.project.root.join("data/nested/assets/empty")).unwrap();
    fs::write(fixture.project.root.join("data/nested/assets/file.bin"), b"nested saved bytes").unwrap();
    fs::write(&descriptor, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    fixture.project = crate::project::open(&fixture.project.root).unwrap();
    // bind_metadata要求未交付的独占租约；此处重建字节夹具版本，不修改现有活动包。
    let executable = fixture.root.join("nested_game.exe");
    fs::write(&executable, b"nested runtime fixture").unwrap();
    let source_identity = crate::code_iteration::export_source_identity(&fixture.project, &fixture.target, &BuildControl::default()).unwrap();
    let bundle = build_artifacts::publish(&fixture.target, &fixture.project.context.storage_key, &executable, &[],
        serde_json::json!({"project_id":fixture.project.id,"project_root":fixture.project.root,"profile":"render-dev","export_source_identity":source_identity}), &BuildControl::default()).unwrap();
    fixture.bundle = bundle;
    fixture.export(&fixture.output(), &BuildControl::default()).unwrap();
    assert!(fixture.output().join("data/nested/assets/empty").is_dir());
    assert_eq!(fs::read(fixture.output().join("data/nested/assets/file.bin")).unwrap(), b"nested saved bytes");
}

#[cfg(windows)]
#[test]
fn locked_stage_directory_rolls_back_previous_package_after_rename_failure() {
    use std::os::windows::fs::OpenOptionsExt;
    let fixture = Fixture::new();
    let control = BuildControl::default();
    fixture.export(&fixture.output(), &control).unwrap();
    let original = fs::read(fixture.output().join(manifest::MARKER)).unwrap();
    let previous = transaction::existing(&fixture.output(), &fixture.project, &control).unwrap();
    let plan = files::Plan::collect(&fixture.project, &fixture.bundle, &control).unwrap();
    let mut stage = transaction::Stage::create(&fixture.output()).unwrap();
    let (progress, _) = mpsc::sync_channel(32);
    plan.copy(&stage.path, &control, &progress).unwrap();
    manifest::write(&fixture.project, &fixture.bundle, &plan, &stage.path, &control).unwrap();
    // 允许读写以保持真实摘要校验，唯独不共享DELETE，使新目录rename实际失败。
    let lock = fs::OpenOptions::new().read(true).share_mode(0x1 | 0x2)
        .custom_flags(0x02000000) // FILE_FLAG_BACKUP_SEMANTICS：打开真实目录句柄。
        .open(&stage.path).unwrap();
    let error = stage.publish(&fixture.output(), previous.as_ref(), &fixture.project, &control).unwrap_err();
    assert!(error.contains("旧导出已恢复"), "{error}");
    assert_eq!(fs::read(fixture.output().join(manifest::MARKER)).unwrap(), original);
    let stage_path = stage.path.clone();
    drop(lock);
    drop(stage);
    assert!(!stage_path.exists());
    assert!(!fs::read_dir(&fixture.project.root).unwrap().any(|entry| entry.unwrap().file_name().to_string_lossy().starts_with(".feathrise-export-backup-")));
}
