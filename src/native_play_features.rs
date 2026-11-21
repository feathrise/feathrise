//! 工程启动使用工程声明的 Cargo 默认组和显式选择，独立导出沿用发布能力。

use super::ProjectBuildFeatures;
use std::{collections::BTreeSet, process::Command};

// 仅独立导出沿用编辑器发布版本的能力合同。
const GAME_FEATURES: [(&str, bool); 7] = [
    ("raytracing", cfg!(feature = "raytracing")),
    ("rtx_rendering", cfg!(feature = "rtx_rendering")),
    (
        "dlss_frame_generation",
        cfg!(feature = "dlss_frame_generation"),
    ),
    ("fsr3_upscaling", cfg!(feature = "fsr3_upscaling")),
    (
        "fsr3_frame_generation",
        cfg!(feature = "fsr3_frame_generation"),
    ),
    ("physics_debug", cfg!(feature = "physics_debug")),
    ("initial_items", cfg!(feature = "initial_items")),
];

fn declared(manifest: &toml::Value) -> impl Iterator<Item = &str> {
    manifest.get("features").and_then(toml::Value::as_table).into_iter().flatten()
        .filter_map(|(name, value)| value.is_array().then_some(name.as_str()))
}

pub(crate) fn catalog(path: &std::path::Path) -> Result<Vec<String>, String> {
    let manifest = crate::project_template::read_cargo_manifest(path)?;
    let mut names: Vec<_> = declared(&manifest).map(str::to_owned).collect();
    names.sort_unstable();
    Ok(names)
}

pub(super) fn export_selection() -> ProjectBuildFeatures {
    let names = GAME_FEATURES.into_iter().filter(|(_, enabled)| *enabled)
        .map(|(name, _)| name.to_owned()).collect();
    ProjectBuildFeatures::new(false, names)
}

pub(super) fn validate(manifest: &toml::Value, selected: &ProjectBuildFeatures) -> Result<(), String> {
    for name in selected.names() {
        if !manifest.get("features").and_then(|table| table.get(name)).is_some_and(toml::Value::is_array) {
            return Err(format!("工程 Cargo.toml 未声明特性“{name}”，请在「特性」菜单中重新选择"));
        }
    }
    Ok(())
}

/// 工程别名也可能间接打开厂商桥；保留这些组合原有的 Cargo 完整构建路径。
pub(super) fn requires_native(manifest: &toml::Value, selected: &ProjectBuildFeatures) -> bool {
    let mut pending: Vec<_> = selected.names().iter().map(String::as_str).collect();
    if selected.default_enabled() { pending.push("default"); }
    let mut seen = BTreeSet::new();
    let table = manifest.get("features").and_then(toml::Value::as_table);
    while let Some(name) = pending.pop() {
        if matches!(name.rsplit('/').next(), Some("rtx_rendering" | "dlss_frame_generation" | "fsr3_frame_generation")) {
            return true;
        }
        if seen.insert(name)
            && let Some(values) = table.and_then(|table| table.get(name)).and_then(toml::Value::as_array)
        {
            pending.extend(values.iter().filter_map(toml::Value::as_str));
        }
    }
    false
}

pub(super) fn apply(command: &mut Command, selected: &ProjectBuildFeatures) {
    // 勾选 default 时由 Cargo 展开工程默认组，命令与普通 cargo run 保持一致。
    if !selected.default_enabled() { command.arg("--no-default-features"); }
    if !selected.names().is_empty() {
        command.args(["--features", &selected.names().join(",")]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_features_follow_the_cargo_default_switch() {
        let manifest = toml::from_str("[features]\ndefault = ['raytracing']\nraytracing = []\ncustom = []").unwrap();
        for (selected, expected) in [
            (ProjectBuildFeatures::default(), &[][..]),
            (ProjectBuildFeatures::new(false, vec![]), &["--no-default-features"][..]),
            (ProjectBuildFeatures::new(true, vec!["custom".into()]), &["--features", "custom"][..]),
            (ProjectBuildFeatures::new(false, vec!["custom".into()]), &["--no-default-features", "--features", "custom"][..]),
            (ProjectBuildFeatures::new(false, vec!["raytracing".into()]), &["--no-default-features", "--features", "raytracing"][..]),
        ] {
            validate(&manifest, &selected).unwrap();
            let mut command = Command::new("cargo");
            apply(&mut command, &selected);
            assert_eq!(command.get_args().collect::<Vec<_>>(), expected);
        }
        validate(&toml::Value::Table(Default::default()), &ProjectBuildFeatures::default()).unwrap();
    }

    #[test]
    fn project_features_follow_default_aliases_for_native_builds_and_reject_removed_entries() {
        let manifest = toml::from_str("[features]\ndefault = ['alias']\nalias = ['vendor']\nvendor = ['feathrise_runtime/fsr3_frame_generation']").unwrap();
        assert!(declared(&manifest).any(|name| name == "default"));
        assert!(requires_native(&manifest, &ProjectBuildFeatures::default()));
        assert!(!requires_native(&manifest, &ProjectBuildFeatures::new(false, vec![])));
        assert!(requires_native(&manifest, &ProjectBuildFeatures::new(false, vec!["alias".into()])));
        assert!(validate(&manifest, &ProjectBuildFeatures::new(true, vec!["missing".into()])).is_err());
    }

    #[test]
    fn complete_game_capability_contract_is_declared_by_the_project_template() {
        // 模板保留各发布版本用于导出的工程特性名。
        let manifest = include_str!("../templates/empty/Cargo.toml");
        let mut command = Command::new("cargo");
        let all = ProjectBuildFeatures::new(false, GAME_FEATURES.into_iter().map(|(name, _)| name.to_owned()).collect());
        apply(&mut command, &all);
        let args = command
            .get_args()
            .map(|arg| arg.to_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(args.len(), 3);
        assert_eq!(&args[..2], ["--no-default-features", "--features"]);
        for feature in args[2].split(',') {
            assert!(
                manifest
                    .lines()
                    .any(|line| line.starts_with(&format!("{feature} = ["))),
                "工程模板未声明{feature}"
            );
        }
        for alias in ["dlss", "fsr"] {
            assert!(
                manifest
                    .lines()
                    .any(|line| line.starts_with(&format!("{alias} = [")))
            );
        }
    }
}
