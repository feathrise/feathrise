//! 路径选择使用显式输入，测试不修改全局 cwd 或环境，避免并行测试互相污染。

use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

pub(super) struct Fixture(pub(super) PathBuf);

impl Fixture {
    pub(super) fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "feathrise-resource-paths-{}-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&path).unwrap();
        Self(path.canonicalize().unwrap())
    }

    fn payload(&self, name: &str) -> PathBuf {
        let root = self.0.join(name);
        for relative in required_files(RESOURCE_MANIFEST).unwrap() {
            let path = root.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, b"bounded resource fixture").unwrap();
        }
        root
    }

    fn directory(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::create_dir_all(&path).unwrap();
        path
    }
}

#[test]
fn resource_manifest_is_single_bounded_safe_source_for_startup_and_packaging() {
    let files = required_files(RESOURCE_MANIFEST).unwrap();
    assert_eq!(files.len(), 29);
    assert!(files.iter().any(|path| path == "assets/templates/empty/Cargo.toml"));
    // 可选 Blender 工具只接受运行时显式输入，不能成为启动或分发的必需内嵌资源。
    assert!(files.iter().all(|path| !path.starts_with("assets/blender/")));
    for bad in [
        r#"{"format":2,"required_files":["a"]}"#,
        r#"{"format":1,"required_files":[]}"#,
        r#"{"format":1,"required_files":["../escape"]}"#,
        r#"{"format":1,"required_files":["C:/absolute"]}"#,
        r#"{"format":1,"required_files":["a","A"]}"#,
        r#"{"format":1,"required_files":["a"],"unknown":true}"#,
    ] {
        assert!(required_files(bad).is_err(), "{bad}");
    }
}

#[test]
fn development_mode_requires_both_original_output_and_source_root() {
    let fixture = Fixture::new();
    let source = fixture.payload("source");
    let output = fixture.directory("target/debug");
    let moved = fixture.directory("installed");
    assert_eq!(resource_mode(&source, &output.join("feathrise.exe"), &source, &output), ResourceMode::Development);
    assert_eq!(resource_mode(&source, &moved.join("feathrise.exe"), &source, &output), ResourceMode::Installed);
    assert_eq!(resource_mode(&moved, &output.join("feathrise.exe"), &source, &output), ResourceMode::Installed);
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // 仅清理由本测试 create_dir 成功创建且仍在临时根内的唯一目录。
        if self.0.starts_with(std::env::temp_dir().canonicalize().unwrap()) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}

#[test]
fn moved_executable_ignores_old_adjacent_payload_and_uses_embedded_version() {
    let fixture = Fixture::new();
    let installed = fixture.payload("old-install");
    let cache = fixture.payload("embedded-cache");
    let root = resolve(
        &installed.join("feathrise.exe"), None, None,
        &fixture.0.join("missing-source"), &fixture.0.join("missing-target"),
        || Ok(cache.clone()),
    ).unwrap();
    assert_eq!(root, cache);
    assert_ne!(root, installed);
}

#[test]
fn cargo_output_uses_source_payload_regardless_of_working_directory() {
    let fixture = Fixture::new();
    let source = fixture.payload("source");
    let custom_output = fixture.directory("existing-cache/render-dev");
    // 输入合同没有 cwd：不在源码目录启动、或者 Cargo 使用自定义 target-dir，结果相同。
    assert_eq!(resolve(
        &custom_output.join("feathrise.exe"), None, None, &source, &custom_output, || panic!("development must not extract"),
    ).unwrap(), source);
}

#[test]
fn moved_bare_executable_never_falls_back_to_existing_build_machine_source() {
    let fixture = Fixture::new();
    let source = fixture.payload("source");
    let built = fixture.directory("target/debug");
    let moved = fixture.directory("installed");
    let cache = fixture.payload("cache");
    let selected = resolve(&moved.join("feathrise.exe"), None, None, &source, &built, || Ok(cache.clone())).unwrap();
    assert_eq!(selected, cache);
    assert_ne!(selected, source);
}

#[test]
fn original_output_survives_source_checkout_removal_using_embedded_resources() {
    let fixture = Fixture::new();
    let output = fixture.directory("existing-target/debug");
    let missing_source = fixture.0.join("removed-checkout");
    let cache = fixture.payload("embedded-cache");
    let executable = output.join("feathrise.exe");
    let root = resolve(&executable, None, None, &missing_source, &output, || Ok(cache.clone())).unwrap();
    assert_eq!(root, cache);
    assert_eq!(resource_mode(&root, &executable, &missing_source, &output), ResourceMode::Installed);
}

#[test]
fn executable_next_to_project_assets_does_not_treat_them_as_editor_resources() {
    let fixture = Fixture::new();
    let source = fixture.payload("source");
    let built = fixture.directory("target/debug");
    let moved = fixture.directory("project");
    fs::create_dir(moved.join("assets")).unwrap();
    fs::write(moved.join("assets/author.txt"), b"author content").unwrap();
    let cache = fixture.payload("cache");
    let root = resolve(&moved.join("feathrise.exe"), None, None, &source, &built, || Ok(cache.clone())).unwrap();
    assert_eq!(root, cache);
    assert_eq!(fs::read(moved.join("assets/author.txt")).unwrap(), b"author content");
}

#[test]
fn explicit_override_wins_and_its_failure_never_uses_other_candidates() {
    let fixture = Fixture::new();
    let installed = fixture.payload("installed");
    let override_root = fixture.payload("override");
    let missing = fixture.0.join("missing");
    let executable = installed.join("feathrise.exe");
    assert_eq!(resolve(&executable, Some(&override_root), Some(&missing), &missing, &missing, || panic!()).unwrap(), override_root);
    let error = resolve(&executable, Some(&missing), Some(&installed), &installed, &installed, || panic!()).unwrap_err();
    assert!(error.contains("FEATHRISE_EDITOR_ROOT"));
    assert!(error.contains("missing"));
    assert!(resolve(&executable, Some(Path::new("relative")), None, &installed, &installed, || panic!()).unwrap_err().contains("绝对路径"));
    assert!(resolve(&executable, Some(Path::new("")), None, &installed, &installed, || panic!()).is_err());
}

#[test]
fn legacy_override_accepts_original_repository_or_editor_root() {
    let fixture = Fixture::new();
    let source = fixture.payload("repository/feathrise");
    let executable = fixture.0.join("unused/feathrise.exe");
    for legacy in [&source, source.parent().unwrap()] {
        assert_eq!(resolve(&executable, None, Some(legacy), &source, &source, || panic!()).unwrap(), source);
    }
    let missing = fixture.0.join("missing-legacy");
    assert!(resolve(&executable, None, Some(&missing), &source, &source, || panic!()).unwrap_err().contains("EDITOR_HOST_REPO_ROOT"));
}

#[test]
fn required_resource_failures_name_the_exact_file_and_never_load_whole_payload() {
    let fixture = Fixture::new();
    let source = fixture.payload("resources");
    fs::write(source.join("assets/icons/editor/tree_scene.png"), []).unwrap();
    fs::remove_file(source.join("assets/hotpatch/原生热补丁链接代理.rs")).unwrap();
    let error = validate_root(&source, "测试资源").unwrap_err();
    assert!(error.contains("tree_scene.png：必须是非空普通文件"));
    assert!(error.contains("assets/hotpatch/原生热补丁链接代理.rs"));
}
