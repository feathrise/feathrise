//! 首次完整fat版本沿用原运行副本；冻结EXE/PDB和输入摘要，为同一World提供不可变补丁基线。

use super::{PreparedGame, DiagnosticCallback, features, rust_runtime_directory};
use crate::{build_artifacts, build_process::BuildControl, code_iteration::{capture_source, hotpatch::{self, FatBaseline}}, project::Project};
use feathrise_editor_core::ProjectBuildProfile;
use std::{fs, path::Path, sync::Arc};

pub(super) fn prepare(project: &Project, target: &Path, profile: ProjectBuildProfile,
    control: &BuildControl, diagnostics: Option<DiagnosticCallback>) -> Result<Option<PreparedGame>, String> {
    // 首轮只验收已验证的原生渲染组合；厂商运行库继续由原Cargo回执链发布。
    if profile != ProjectBuildProfile::Debug || cfg!(any(feature = "rtx_rendering", feature = "dlss_frame_generation", feature = "fsr3_frame_generation")) {
        return Ok(None);
    }
    let manifest = fs::read_to_string(&project.manifest).map_err(|error| error.to_string())?;
    let manifest: toml::Value = toml::from_str(&manifest).map_err(|error| error.to_string())?;
    // dx首轮只替换入口bin；Cargo隐式lib属于所选叶包，不能按外层工程根漏掉嵌套lib后将其误判为bin补丁。
    let package_root = project.manifest.parent().ok_or("热更新Cargo入口缺少包目录")?;
    if manifest.get("lib").is_some() || package_root.join("src/lib.rs").is_file() { return Ok(None); }
    if manifest.get("features").and_then(|features| features.get("function_hotpatch")).is_none()
        || !matches!(hotpatch::compatibility::Contract::parse(&toml::to_string(&manifest).map_err(|error| error.to_string())?), Ok(Some(_))) {
        return Ok(None);
    }
    let source = capture_source(project, target, control)?;
    if source.hotpatch_shape.is_none() { return Ok(None); }
    let dx = std::env::var_os("FEATHRISE_DX_PATH").map(std::path::PathBuf::from)
        .unwrap_or_else(|| crate::project_template_root().parent().unwrap_or(Path::new(".")).join(".local/hotpatch/tools/dx-0.7.9/dx.exe"));
    if !dx.is_file() {
        if let Some(log) = control.log_sink() { log.task_message("未安装函数热更新工具，本次使用完整编译与重启路径"); }
        return Ok(None);
    }
    let mut features = features::identity();
    if !features.is_empty() { features.push(','); }
    features.push_str("function_hotpatch");
    let fat = hotpatch::toolchain::fat(project, target, control, &features, diagnostics)?;
    if source != capture_source(project, target, control)? { return Err("完整热更新基线构建期间源码变化，请等待最新编译".into()); }
    let dlls = hotpatch::toolchain::runtime_dlls(&fat, target, control)?;
    let profile_dir = fat.artifacts.executable.parent().ok_or("fat产物缺少父目录")?;
    let search = vec![profile_dir.to_owned(), profile_dir.join("deps"), rust_runtime_directory(project, control)?];
    let mut files = build_artifacts::runtime_files(&fat.artifacts.executable, target, &dlls, &search, false, control)?;
    files.push(build_artifacts::RuntimeFile { source: fat.artifacts.pdb.clone(),
        relative: fat.artifacts.pdb.file_name().ok_or("fat产物缺少PDB名")?.into() });
    let bundle = build_artifacts::publish(target, &project.context.storage_key, &fat.artifacts.executable, &files,
        serde_json::json!({"project_id":project.id,"profile":"render-dev","kind":"function-hotpatch-fat"}), control)?;
    let base_build = build_artifacts::hash_file(&bundle.executable, control)?.0;
    let patch_root = bundle.directory.join("code-patches");
    fs::create_dir(&patch_root).map_err(|error| error.to_string())?;
    let baseline = Arc::new(FatBaseline {
        client_json: fat.client_for_frozen_executable(&bundle.executable)?, source, base_build,
        directory: bundle.directory.clone(), patch_root, features, bundle: bundle.clone(),
    });
    Ok(Some(PreparedGame {
        executable: bundle.executable.clone(), target: target.to_owned(), linked_paths: vec![bundle.directory.clone()],
        dlls, build_outputs: Vec::new(), project_id: project.id.clone(), project_root: project.root.clone(),
        profile, bundle: Some(bundle), hotpatch: Some(baseline),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_manifest_implicit_library_keeps_complete_restart_without_running_dx() {
        let root = std::env::temp_dir().join(format!("feathrise-nested-lib-{}", bevy::asset::uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let root = root.canonicalize().unwrap();
        struct Fixture(std::path::PathBuf);
        impl Drop for Fixture {
            fn drop(&mut self) {
                assert_eq!(self.0.parent(), Some(std::env::temp_dir().canonicalize().unwrap().as_path()));
                assert!(self.0.file_name().unwrap().to_string_lossy().starts_with("feathrise-nested-lib-"));
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let fixture = Fixture(root);
        fs::create_dir_all(fixture.0.join("game/src")).unwrap();
        fs::write(fixture.0.join("Cargo.toml"), "[workspace]\nmembers=['game']\nresolver='3'\n").unwrap();
        fs::write(fixture.0.join("game/Cargo.toml"), "[package]\nname='owned_nested_lib'\nversion='0.1.0'\n[features]\nfunction_hotpatch=[]\n[package.metadata.feathrise.hotpatch]\nenabled=true\nfunctions=['game/src/lib.rs::live_step']\n").unwrap();
        fs::write(fixture.0.join("game/src/main.rs"), "fn main() {}\n").unwrap();
        fs::write(fixture.0.join("game/src/lib.rs"), "pub fn live_step() -> f32 { 1.0 }\n").unwrap();
        fs::write(fixture.0.join("editor-project.json"), r#"{"format":1,"id":"owned-nested-lib","name":"自有嵌套库","cargo_manifest":"game/Cargo.toml","package":"owned_nested_lib"}"#).unwrap();
        let project = crate::project::open(&fixture.0).unwrap();
        assert!(!project.root.join("src/lib.rs").exists());
        assert!(project.manifest.parent().unwrap().join("src/lib.rs").is_file());
        // 先撤销构建租约：正确的lib门须在任何源码扫描/工具准备前返回None，错过此门只能报取消，绝不执行dx。
        let control = BuildControl::default();
        control.cancel();
        assert!(prepare(&project, &fixture.0.join("unused-target"), ProjectBuildProfile::Debug, &control, None)
            .is_ok_and(|prepared| prepared.is_none()));
        assert!(!fixture.0.join("unused-target").exists());
    }
}
