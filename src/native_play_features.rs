//! 宿主向工程预览和独立启动转发游戏能力，默认特性一致时复用普通 Cargo 开发产物。

use std::process::Command;

// 只转发宿主编译时已经启用的游戏能力，不读取环境变量、用户字符串或作者工具特性。
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

pub(super) fn apply(command: &mut Command, native_debug_manifest: Option<&toml::Value>) {
    append(command, GAME_FEATURES, native_debug_manifest);
}

/// 构建身份与实际命令共用同一特性表，不能在监听器维护另一份可能漂移的能力列表。
pub(super) fn identity() -> String {
    GAME_FEATURES.into_iter().filter_map(|(name, enabled)| enabled.then_some(name)).collect::<Vec<_>>().join(",")
}

fn append(command: &mut Command, features: [(&str, bool); 7], native_debug_manifest: Option<&toml::Value>) {
    let enabled = features
        .into_iter()
        .filter_map(|(name, active)| active.then_some(name))
        .collect::<Vec<_>>();
    // Cargo 会把名为 default 的特性也计入构建指纹。原生 Debug 只有能力完全一致时才保留它。
    if native_debug_manifest.is_some_and(|manifest| defaults_match(manifest, &enabled)) {
        return;
    }
    command.arg("--no-default-features");
    if !enabled.is_empty() {
        command.args(["--features", &enabled.join(",")]);
    }
}

fn defaults_match(manifest: &toml::Value, enabled: &[&str]) -> bool {
    let Some(defaults) = manifest.get("features").and_then(|features| features.get("default")) else {
        return enabled.is_empty();
    };
    defaults.as_array().is_some_and(|defaults| {
        defaults.len() == enabled.len()
            && enabled.iter().all(|&name| defaults.iter().any(|feature| feature.as_str() == Some(name)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_host_game_capabilities_never_reenable_project_defaults_or_pass_empty_features() {
        let mut command = Command::new("cargo");
        let manifest = toml::from_str("[features]\ndefault = ['raytracing']\nraytracing = []").unwrap();
        append(&mut command, GAME_FEATURES.map(|(name, _)| (name, false)), Some(&manifest));
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            ["--no-default-features"]
        );
    }

    #[test]
    fn complete_game_capability_contract_is_declared_by_the_project_template() {
        // 模板漏一个名字就会让显式功能的工程预览构建失败。
        let manifest = include_str!("../assets/templates/empty/Cargo.toml");
        let mut command = Command::new("cargo");
        append(&mut command, GAME_FEATURES.map(|(name, _)| (name, true)), None);
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
