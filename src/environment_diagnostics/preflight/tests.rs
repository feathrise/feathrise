//! 预检专项回归：使用本轮临时配置，核对配置来源、工作目录及探针预算。

use super::*;
use crate::environment_diagnostics::preflight::toolchains::{Linker, configured_linker};
use std::{fs, path::PathBuf};

#[test]
fn linker_diagnostics_resolve_relative_paths_from_the_config_owner() {
    let root = std::env::temp_dir().join(format!(
        "feathrise-tool-preflight-{}",
        bevy::asset::uuid::Uuid::new_v4()
    ));
    fs::create_dir(&root).unwrap();
    fs::create_dir(root.join(".cargo")).unwrap();
    fs::create_dir(root.join("nested")).unwrap();
    fs::write(
        root.join(".cargo/config.toml"),
        "[target.feathrise-fixture-target]\nlinker = \"tools/custom-linker\"\n",
    )
    .unwrap();
    let linker = configured_linker(&root.join("nested"), "feathrise-fixture-target").unwrap();
    let Linker::Explicit(linker) = linker else {
        panic!("应找到夹具的上层链接器配置");
    };
    assert_eq!(linker, root.join("tools/custom-linker"));
    fs::remove_file(root.join(".cargo/config.toml")).unwrap();
    fs::remove_dir(root.join(".cargo")).unwrap();
    fs::remove_dir(root.join("nested")).unwrap();
    fs::remove_dir(root).unwrap();
}

#[test]
fn probe_is_read_only_and_uses_the_selected_cargo_working_directory() {
    let control = BuildControl::default();
    let probe = Probe {
        control: &control,
        deadline: Instant::now(),
        locale: EditorLocale::English,
    };
    let directory = PathBuf::from("workspace/manifest");
    let command = probe.command("cargo", &directory);
    assert_eq!(command.get_current_dir(), Some(directory.as_path()));
    assert!(command.get_envs().any(
        |(key, value)| key == "RUSTUP_AUTO_INSTALL" && value.is_some_and(|value| value == "0")
    ));
    assert!(command.get_envs().any(
        |(key, value)| key == "CARGO_NET_OFFLINE" && value.is_some_and(|value| value == "true")
    ));
    assert!(
        probe.output(&command).unwrap_err().contains("time limit"),
        "过期的探针不得尝试创建工具进程"
    );
    assert!(!control.cleanup_failed());
}

#[test]
#[ignore = "只由预检超时回归启动并在专属进程树中收口"]
fn slow_probe_process() {
    std::thread::sleep(Duration::from_secs(10));
}

#[test]
fn timed_out_probe_confirms_its_owned_process_tree_exited() {
    let control = BuildControl::default();
    let probe = Probe {
        control: &control,
        deadline: Instant::now() + Duration::from_millis(200),
        locale: EditorLocale::Chinese,
    };
    let exe = std::env::current_exe().unwrap();
    let mut command = probe.command(&exe, exe.parent().unwrap());
    command.args([
        "--ignored",
        "--exact",
        "environment_diagnostics::preflight::tests::slow_probe_process",
    ]);
    let error = probe.output(&command).unwrap_err();
    assert!(error.contains("超时"), "{error}");
    assert!(!control.cleanup_failed());
}
