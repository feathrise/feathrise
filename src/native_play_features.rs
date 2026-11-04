//! 宿主与F12游戏使用同一固定能力合同；编译与Cargo运行必须同时禁用工程隐式默认功能。

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

/// build和run唯一共同入口；没有任何已启用能力时也必须阻止工程默认raytracing被重新打开。
pub(super) fn apply(command: &mut Command) {
    append(command, GAME_FEATURES);
}

/// 构建身份与实际命令共用同一特性表，不能在监听器维护另一份可能漂移的能力列表。
pub(super) fn identity() -> String {
    GAME_FEATURES.into_iter().filter_map(|(name, enabled)| enabled.then_some(name)).collect::<Vec<_>>().join(",")
}

fn append(command: &mut Command, features: [(&str, bool); 7]) {
    command.arg("--no-default-features");
    let enabled = features
        .into_iter()
        .filter_map(|(name, active)| active.then_some(name))
        .collect::<Vec<_>>()
        .join(",");
    if !enabled.is_empty() {
        command.args(["--features", &enabled]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_host_game_capabilities_never_reenable_project_defaults_or_pass_empty_features() {
        let mut command = Command::new("cargo");
        append(&mut command, GAME_FEATURES.map(|(name, _)| (name, false)));
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            ["--no-default-features"]
        );
    }

    #[test]
    fn complete_game_capability_contract_is_declared_by_the_project_template() {
        // 模板漏一个名字就会让显式功能的F12构建失败；依赖特性不改变时无需跑厂商硬件矩阵。
        let manifest = include_str!("../assets/templates/empty/Cargo.toml");
        let mut command = Command::new("cargo");
        append(&mut command, GAME_FEATURES.map(|(name, _)| (name, true)));
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
