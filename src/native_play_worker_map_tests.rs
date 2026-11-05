//! F12后台边界回归：目标校验与运行版本身份绑定，不让坏图、共享租约或失败尝试污染下一次运行。

use super::{BuildControl, Project, validate_requested_map, verify_game_contract};
use feathrise_map_content::{MAP_CATALOG_ASSET_PATH, MAP_MANIFEST_FILE_NAME};
use serde_json::json;
use std::{fs, path::PathBuf};

struct Fixture {
    root: PathBuf,
    project: Project,
}

impl Fixture {
    fn new() -> Self {
        Self::with_package("new_editor_game_template")
    }

    fn with_package(package: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "saved-play-map-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let root = root.canonicalize().unwrap();
        fs::create_dir_all(root.join("assets/maps/SELECTED")).unwrap();
        fs::create_dir_all(root.join("assets/maps/UNRELATED")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            format!("[package]\nname='{package}'\nversion='0.1.0'\nedition='2024'\n"),
        )
        .unwrap();
        fs::write(root.join("editor-project.json"), serde_json::to_vec(&json!({
            "format":2,"sdk_abi":crate::project::LEGACY_SDK_ABI,
            "id":"26c5f149-a3ca-43a8-9cc1-741eb25c0298","name":"F12目标回归",
            "package":package,"cargo_manifest":"Cargo.toml",
            "asset_roots":["assets"],"editor_data":".editor","game_entry":{"kind":"saved-world","version":1}
        })).unwrap()).unwrap();
        fs::write(root.join("assets").join(MAP_CATALOG_ASSET_PATH), serde_json::to_vec(&json!({"schema_version":1,"maps":[
            {"id":"SELECTED","display_name":"有效目标","folder":"maps/SELECTED","published":true},
            {"id":"UNRELATED","display_name":"无关坏图","folder":"maps/UNRELATED","published":false}
        ]})).unwrap()).unwrap();
        fs::write(
            root.join("assets/maps/UNRELATED")
                .join(MAP_MANIFEST_FILE_NAME),
            b"unrelated invalid JSON",
        )
        .unwrap();
        fs::write(
            root.join("author-draft.sentinel"),
            b"author draft stays intact",
        )
        .unwrap();
        let project = crate::project::open(&root).unwrap();
        let fixture = Self { root, project };
        fixture.write_target("SELECTED");
        fixture
    }

    fn target(&self) -> PathBuf {
        self.root
            .join("assets/maps/SELECTED")
            .join(MAP_MANIFEST_FILE_NAME)
    }

    fn write_target(&self, id: &str) {
        fs::write(self.target(), serde_json::to_vec(&json!({
            "schema_version":1,"id":id,"display_name":"有效目标","runtime_scene":"DynamicMapScene",
            "world_min":[-32.,-32.],"world_max":[32.,32.],"default_spawn":[0.,2.,6.],"terrains":[],"asset_roots":["scene"]
        })).unwrap()).unwrap();
    }
}

#[test]
fn saved_play_accepts_old_current_and_custom_package_names_without_migrating_author_files() {
    // 包名归作者工程：模板改名只影响未来新建，不成为打开或F12合同的白名单。
    for package in ["new_editor_game_template", "feathrise_game_template", "author-custom-game"] {
        let fixture = Fixture::with_package(package);
        fs::create_dir(fixture.root.join("src")).unwrap();
        fs::write(fixture.root.join("src/main.rs"), b"fn main() { /* author source */ }").unwrap();
        fs::write(fixture.root.join("Cargo.lock"), b"# author-owned lockfile\nversion = 4\n").unwrap();
        let paths = [
            "Cargo.toml", "Cargo.lock", "editor-project.json", "src/main.rs",
            "author-draft.sentinel",
        ];
        let before: Vec<_> = paths.iter().map(|path| fs::read(fixture.root.join(path)).unwrap()).collect();
        let project = crate::project::open(&fixture.root).unwrap();
        assert_eq!(project.package, package);
        verify_game_contract(&project, &BuildControl::default()).unwrap();
        validate_requested_map(&project, "SELECTED", &BuildControl::default()).unwrap();
        assert_eq!(project, fixture.project);
        for (path, bytes) in paths.iter().zip(before) {
            assert_eq!(fs::read(fixture.root.join(path)).unwrap(), bytes, "{package}: {path}");
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let temporary = std::env::temp_dir().canonicalize().unwrap();
        assert!(self.root.starts_with(temporary));
        assert!(
            self.root
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("saved-play-map-")
        );
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn requested_target_ignores_bad_unrelated_map_and_absent_unused_presets() {
    let fixture = Fixture::new();
    let before = fs::read(fixture.target()).unwrap();
    assert!(!fixture.root.join("assets/config").exists());
    validate_requested_map(&fixture.project, "SELECTED", &BuildControl::default()).unwrap();
    assert_eq!(fs::read(fixture.target()).unwrap(), before);
    assert_eq!(
        fs::read(
            fixture
                .root
                .join("assets/maps/UNRELATED")
                .join(MAP_MANIFEST_FILE_NAME)
        )
        .unwrap(),
        b"unrelated invalid JSON"
    );
}

#[test]
fn missing_or_wrong_target_rejects_then_retries_after_repair_without_writing_author_state() {
    let fixture = Fixture::new();
    let before = fs::read(fixture.root.join("author-draft.sentinel")).unwrap();
    fs::remove_file(fixture.target()).unwrap();
    assert!(
        validate_requested_map(&fixture.project, "SELECTED", &BuildControl::default()).is_err()
    );
    fixture.write_target("WRONG-MAP");
    assert!(
        validate_requested_map(&fixture.project, "SELECTED", &BuildControl::default()).is_err()
    );
    assert!(
        validate_requested_map(&fixture.project, "UNREGISTERED", &BuildControl::default()).is_err()
    );
    fixture.write_target("SELECTED");
    validate_requested_map(&fixture.project, "SELECTED", &BuildControl::default()).unwrap();
    assert_eq!(
        fs::read(fixture.root.join("author-draft.sentinel")).unwrap(),
        before
    );
}

#[test]
fn cancelled_validation_does_not_poison_the_next_attempt_control() {
    let fixture = Fixture::new();
    let cancelled = BuildControl::default();
    cancelled.cancel();
    assert!(validate_requested_map(&fixture.project, "SELECTED", &cancelled).is_err());
    let next_attempt = BuildControl::default();
    validate_requested_map(&fixture.project, "SELECTED", &next_attempt).unwrap();
    assert!(!next_attempt.is_cancelled());
    assert!(!next_attempt.cleanup_failed());
}
#[test]
fn both_cargo_actions_use_the_frozen_profile_and_current_project_cache() {
    use feathrise_editor_core::ProjectBuildProfile;
    let fixture = Fixture::new();
    let target = crate::writable_paths::project_target(&fixture.project.root, None).unwrap();
    assert_eq!(target, fixture.project.root.join("target"));
    for action in ["build", "run"] {
        for (profile, expected) in [(ProjectBuildProfile::Debug, "render-dev"), (ProjectBuildProfile::Release, "release")] {
            let mut command = std::process::Command::new("unused-cargo");
            command.arg(action);
            super::configure_project_cargo(&mut command, &fixture.project, &target, profile);
            let args: Vec<_> = command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect();
            assert_eq!(args[0], action);
            assert_eq!(args.windows(2).find(|args| args[0] == "--profile").unwrap()[1], expected);
            assert_eq!(args.iter().filter(|arg| arg.as_str() == "--profile").count(), 1);
            assert_eq!(command.get_current_dir(), Some(fixture.project.root.as_path()));
            assert!(command.get_envs().any(|(key, value)| key == "CARGO_TARGET_DIR" && value == Some(target.as_os_str())));
        }
    }
}

#[test]
fn switching_projects_keeps_real_cargo_outputs_in_each_projects_target() {
    use feathrise_editor_core::ProjectBuildProfile;
    let first = Fixture::new();
    let second = Fixture::new();
    for fixture in [&first, &second] {
        // 只在本轮独占的无依赖工程运行真实Cargo检查，避免把参数断言冒充落盘验证。
        fs::write(&fixture.project.manifest, format!(
            "[package]\nname='{}'\nversion='0.1.0'\nedition='2024'\n[profile.render-dev]\ninherits='dev'\n",
            fixture.project.package,
        )).unwrap();
        fs::write(fixture.root.join("Cargo.lock"), format!(
            "version = 4\n[[package]]\nname = '{}'\nversion = '0.1.0'\n", fixture.project.package,
        )).unwrap();
        fs::create_dir(fixture.root.join("src")).unwrap();
        fs::write(fixture.root.join("src/main.rs"), "// 本轮缓存归属夹具，无外部依赖。\nfn main() {}\n").unwrap();
    }
    let mut targets = Vec::new();
    for fixture in [&first, &second, &first] {
        let target = crate::writable_paths::project_target(&fixture.project.root, None).unwrap();
        let source = fs::read(fixture.root.join("src/main.rs")).unwrap();
        let draft = fs::read(fixture.root.join("author-draft.sentinel")).unwrap();
        let mut command = std::process::Command::new(env!("CARGO"));
        command.args(["check", "--offline"]);
        super::configure_project_cargo(&mut command, &fixture.project, &target, ProjectBuildProfile::Debug);
        let output = command.output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        assert_eq!(target, fixture.project.root.join("target"));
        assert!(fs::read_dir(target.join("render-dev/deps")).unwrap().any(|entry|
            entry.unwrap().path().extension().is_some_and(|extension| extension == "rmeta")));
        assert_eq!(fs::read(fixture.root.join("src/main.rs")).unwrap(), source);
        assert_eq!(fs::read(fixture.root.join("author-draft.sentinel")).unwrap(), draft);
        targets.push(target);
    }
    assert_ne!(targets[0], targets[1]);
    assert_eq!(targets[0], targets[2]);
}

fn fat_metadata_version(fixture: &Fixture) -> super::PreparedGame {
    use crate::{build_artifacts::{self, RuntimeFile}, code_iteration::{SourceSnapshot, hotpatch::FatBaseline}};
    let executable = fixture.root.join("metadata-unexecuted.exe");
    let pdb = fixture.root.join("metadata-unexecuted.pdb");
    fs::write(&executable, b"owned-unexecuted-fat-executable").unwrap();
    fs::write(&pdb, b"owned-fat-pdb").unwrap();
    // 真实发布两份文件及清单，绝不执行此夹具；两份内部租约与生产fat返回结构一致。
    let bundle = build_artifacts::publish(&fixture.root, &fixture.project.context.storage_key, &executable,
        &[RuntimeFile { source: pdb, relative: "metadata-unexecuted.pdb".into() }], json!({}), &BuildControl::default()).unwrap();
    let mut prepared = super::prepared_for_test(&fixture.project, Default::default());
    prepared.executable = bundle.executable.clone();
    prepared.target = fixture.root.clone();
    prepared.bundle = Some(bundle.clone());
    prepared.hotpatch = Some(std::sync::Arc::new(FatBaseline {
        client_json: std::sync::Arc::new(b"owned-fat-client".to_vec()),
        source: SourceSnapshot { sha256: "owned-source".into(), environment_sha256: "owned-context".into(),
            hotpatch_shape: Some("owned-shape".into()), files: 1, bytes: 1,
            watches: Default::default(), source_watches: Default::default() },
        base_build: "owned-fat-base".into(), directory: bundle.directory.clone(),
        patch_root: bundle.directory.join("code-patches"), features: "function_hotpatch".into(), bundle,
    }));
    prepared
}

fn assert_fat_metadata(prepared: &super::PreparedGame, metadata: serde_json::Value) {
    let bundle = prepared.bundle.as_ref().expect("任何结果都必须恢复本对象的运行副本");
    let baseline = prepared.hotpatch.as_ref().unwrap();
    crate::build_artifacts::verify(bundle, &BuildControl::default()).unwrap();
    crate::build_artifacts::verify(&baseline.bundle, &BuildControl::default()).unwrap();
    assert_eq!(bundle.manifest_sha256, baseline.bundle.manifest_sha256);
    let manifest: serde_json::Value = serde_json::from_slice(&fs::read(bundle.directory.join("runtime-manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["metadata"], metadata);
}

#[test]
fn fat_metadata_internal_handles_bind_export_and_iteration_without_changing_the_baseline() {
    let fixture = Fixture::new();
    let mut prepared = fat_metadata_version(&fixture);
    let baseline = prepared.hotpatch.as_ref().unwrap();
    let identity = (baseline.client_json.clone(), baseline.source.clone(), baseline.base_build.clone(),
        baseline.directory.clone(), baseline.patch_root.clone(), baseline.features.clone());
    let executable = prepared.executable.clone();
    for metadata in [json!({"export_source_identity":"export-input"}), json!({"build_identity":"iteration-input"})] {
        prepared.bind_build_metadata(metadata.clone(), &BuildControl::default()).unwrap();
        assert_fat_metadata(&prepared, metadata);
    }
    let baseline = prepared.hotpatch.as_ref().unwrap();
    assert_eq!(identity, (baseline.client_json.clone(), baseline.source.clone(), baseline.base_build.clone(),
        baseline.directory.clone(), baseline.patch_root.clone(), baseline.features.clone()));
    assert_eq!(prepared.executable, executable);
    assert_eq!(fs::read(&executable).unwrap(), b"owned-unexecuted-fat-executable");
    assert_eq!(fs::read(baseline.directory.join("metadata-unexecuted.pdb")).unwrap(), b"owned-fat-pdb");
}

#[test]
fn fat_metadata_external_baseline_blocks_binding_until_the_owner_releases_it() {
    let fixture = Fixture::new();
    let mut prepared = fat_metadata_version(&fixture);
    let external = prepared.clone();
    assert!(prepared.bind_build_metadata(json!({"attempt":"blocked"}), &BuildControl::default()).is_err());
    assert_fat_metadata(&prepared, json!({}));
    assert_fat_metadata(&external, json!({}));
    drop(external);
    prepared.bind_build_metadata(json!({"attempt":"retry"}), &BuildControl::default()).unwrap();
    assert_fat_metadata(&prepared, json!({"attempt":"retry"}));
}

#[test]
fn fat_metadata_external_bundle_still_blocks_the_original_unique_lease_gate() {
    let fixture = Fixture::new();
    let mut prepared = fat_metadata_version(&fixture);
    let external = prepared.bundle.as_ref().unwrap().clone();
    assert!(prepared.bind_build_metadata(json!({"attempt":"blocked"}), &BuildControl::default()).is_err());
    assert_fat_metadata(&prepared, json!({}));
    crate::build_artifacts::verify(&external, &BuildControl::default()).unwrap();
    drop(external);
    prepared.bind_build_metadata(json!({"attempt":"retry"}), &BuildControl::default()).unwrap();
    assert_fat_metadata(&prepared, json!({"attempt":"retry"}));
}

#[test]
fn fat_metadata_cancellation_restores_both_handles_for_a_fresh_retry() {
    let fixture = Fixture::new();
    let mut prepared = fat_metadata_version(&fixture);
    let cancelled = BuildControl::default();
    cancelled.cancel();
    assert!(prepared.bind_build_metadata(json!({"attempt":"cancelled"}), &cancelled).is_err());
    assert_fat_metadata(&prepared, json!({}));
    prepared.bind_build_metadata(json!({"attempt":"retry"}), &BuildControl::default()).unwrap();
    assert_fat_metadata(&prepared, json!({"attempt":"retry"}));
}
