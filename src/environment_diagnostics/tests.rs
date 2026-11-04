//! 后台失败提示回归：区分环境与作者源码问题，保留退出码，不探测或修改真实环境。

use super::*;

fn failure(stage: CargoStage, diagnostic: &str, offline: bool) -> String {
    let mut command = Command::new("cargo");
    if offline { command.arg("--offline"); }
    cargo_exit_failure(stage, &command, 101, "", diagnostic)
}

#[test]
fn rust_toolchain_and_target_failures_have_specific_manual_repairs() {
    for (diagnostic, advice) in [
        ("error: rustup could not choose a version of cargo to run, because one wasn't specified explicitly, and no default is configured.", "rustup show"),
        ("error: toolchain 'nightly-x86_64-pc-windows-msvc' is not installed", "rust-toolchain.toml"),
        ("error: rustc 1.85.0 is not supported by the following packages", "rustc --version"),
        ("error: could not execute process `rustc -vV` (never executed): os error 2", "rustup target list --installed"),
        ("error[E0463]: can't find crate for `std`", "rustup target list --installed"),
    ] {
        let result = failure(CargoStage::Build, diagnostic, false);
        assert!(result.contains(advice), "{result}");
        assert!(result.contains("退出码101"));
        assert!(result.ends_with(diagnostic));
    }
}

#[test]
fn rust_linker_and_windows_sdk_failures_are_distinguished() {
    let rust_linker = failure(CargoStage::Build, "error: linker `rust-lld` not found", false);
    assert!(rust_linker.contains("rustc --print sysroot"));
    assert!(!rust_linker.contains("Visual Studio Installer"));
    for diagnostic in [
        "error: linker `link.exe` not found",
        "error: failed to find tool \"cl.exe\"",
        "lld-link: error: could not open 'kernel32.lib': no such file or directory",
        "fatal error C1083: Cannot open include file: 'windows.h': No such file or directory",
    ] {
        let result = failure(CargoStage::Build, diagnostic, false);
        assert!(result.contains("Visual Studio Installer"), "{result}");
        assert!(!result.contains("AMD SDK"));
        assert!(!result.contains("CMake"));
    }
}

#[test]
fn lockfile_mismatch_requires_author_confirmation_and_preserves_locked_policy() {
    let result = failure(CargoStage::Build,
        "error: the lock file needs to be updated but --locked was passed to prevent this", false);
    assert!(result.contains("确认依赖修改符合预期"));
    assert!(result.contains("cargo fetch"));
    assert!(result.contains("不会替旧工程重写依赖或删除锁文件"));
    assert!(result.contains("再重试预览或启动"));
    assert!(!result.contains("重试 F12"));
}

#[test]
fn current_cargo_locked_error_reports_manifest_mismatch_and_preserves_original_output() {
    let diagnostic = r"error: cannot update the lock file \\?\D:\自有新工程\Cargo.lock because --locked was passed to prevent this";
    for offline in [false, true] {
        let result = failure(CargoStage::Build, diagnostic, offline);
        assert!(result.contains("Cargo.toml 与 Cargo.lock 当前不一致"), "{result}");
        assert!(result.contains("cargo fetch 更新锁文件并检查差异"));
        assert!(result.contains("编辑器继续使用 --locked"));
        assert!(result.contains("再重试预览或启动"));
        assert!(result.contains("退出码101"));
        assert!(result.ends_with(diagnostic));
    }
    assert!(cargo_suggestion(CargoStage::Build,
        "error: cannot update the lock file Cargo.lock: permission denied", false).is_none(),
        "未指明 --locked 的锁文件 IO 错误不能误报成依赖不一致");
}

#[test]
fn offline_cache_advice_respects_new_and_existing_project_ownership() {
    let diagnostic = "error: failed to download `example v1.0.0`: attempting to make an HTTP request, but --offline was specified";
    let new = failure(CargoStage::NewProject, diagnostic, true);
    assert!(new.contains("SDK 源码目录"));
    assert!(new.contains("cargo fetch --locked"));
    let existing = failure(CargoStage::Build, diagnostic, true);
    assert!(existing.contains("该工程目录显式运行 cargo fetch --locked"));
    assert!(existing.contains("不会擅自联网或升级"));
    let online = failure(CargoStage::Build, "error: failed to download `example v1.0.0`", false);
    assert!(online.contains("网络、代理"));
    assert!(!online.contains("离线模式"));
}

#[test]
fn network_and_git_revision_errors_do_not_echo_sources_in_advice() {
    let url = "https://user:private-token@example.invalid/repository.git";
    for diagnostic in [
        format!("failed to authenticate when downloading repository: {url}"),
        format!("failed to fetch into: {url}; network failure"),
        "revision 0123456789abcdef not found".to_owned(),
        "SSL certificate verify failed".to_owned(),
    ] {
        let result = failure(CargoStage::NewProject, &diagnostic, false);
        let advice = result.split("原始诊断：").next().unwrap();
        assert!(advice.contains("网络、代理"));
        assert!(!advice.contains(url));
        assert!(result.ends_with(&diagnostic));
        assert_eq!(result.matches(url).count(), diagnostic.matches(url).count());
    }
}

#[test]
fn local_sdk_failure_points_to_the_owner_without_fallback() {
    // 新入口与旧工程包名都能得到同一只读建议，自定义依赖仍不误判。
    for package in ["feathrise_runtime", "bevy_feathrise"] {
        let diagnostic = format!("failed to load manifest for dependency `{package}`: failed to read C:/missing/Cargo.toml: The system cannot find the path specified. (os error 3)");
        let new = failure(CargoStage::NewProject, &diagnostic, true);
        assert!(new.contains("sdk-source.toml"));
        let existing = failure(CargoStage::Build, &diagnostic, true);
        assert!(existing.contains("工程自己的 Cargo.toml"));
        assert!(existing.contains("不会修复旧工程"));
        let custom = diagnostic.replace(package, "custom_engine");
        assert!(!failure(CargoStage::Build, &custom, true).contains("处理建议"));
    }
}

#[test]
fn real_cache_io_failures_have_actionable_paths_and_preserve_os_errors() {
    let error = io::Error::from(io::ErrorKind::PermissionDenied);
    let result = cache_failure(Path::new("C:/Program Files/Feathrise/target"), "准备构建缓存", &error);
    assert!(result.contains("C:/Program Files/Feathrise/target"));
    assert!(result.contains(&error.to_string()));
    assert!(result.contains("CARGO_TARGET_DIR"));
    assert!(result.contains("不会自动搬迁或删除旧缓存"));
    for (diagnostic, advice) in [
        ("failed to write target/foo: access is denied (os error 5)", "安装目录不需要开放写权限"),
        ("failed to write target/foo: No space left on device", "磁盘空间不足"),
        ("failed to download `example v1`: No space left on device", "磁盘空间不足"),
        ("failed to open target/foo: file is being used by another process (os error 32)", "占用"),
        ("failed to acquire package cache lock", "不要删除锁文件"),
    ] {
        assert!(failure(CargoStage::Build, diagnostic, false).contains(advice));
    }
    assert!(!failure(CargoStage::Build, "Blocking waiting for file lock on package cache", false).contains("处理建议"));
}

#[test]
fn ordinary_compiler_errors_and_runtime_panics_are_not_environment_failures() {
    for diagnostic in [
        "error[E0308]: mismatched types\n --> src/main.rs:5:9\nexpected `u32`, found `&str`",
        "error[E0308]: mismatched types\n5 | let value: u32 = \"linker `link.exe` not found\";",
        "error[E0425]: cannot find value `name` in this scope",
        "error: expected expression, found `;`\n --> src/main.rs:5:9\n5 | let x = ; // failed to download",
        "error[E0308]: mismatched types\n5 | let value: u32 = \"cannot update the lock file Cargo.lock because --locked was passed to prevent this\";",
        "error: expected expression, found `;`\n --> src/main.rs:5:9\n5 | let x = ; // cannot update the lock file Cargo.lock because --locked was passed to prevent this",
        "error: could not compile `custom_game` due to previous errors",
        "error: custom build command failed with unknown cause",
    ] {
        let result = failure(CargoStage::Build, diagnostic, false);
        assert!(!result.contains("处理建议"), "{result}");
        assert!(result.ends_with(diagnostic));
    }
    let panic = "thread 'main' panicked: failed to download user asset; no default is configured";
    let result = failure(CargoStage::Play, panic, false);
    assert!(result.contains("工程运行失败"));
    assert!(!result.contains("处理建议"));
    assert!(result.ends_with(panic));
}

#[test]
fn combined_diagnostics_keep_unicode_and_both_tails_within_one_budget() {
    let stdout = format!("{}STDOUT_END", "中文".repeat(LOG_BYTES));
    let stderr = format!("{}STDERR_END", "日文".repeat(LOG_BYTES));
    let combined = diagnostic_tail(&stdout, &stderr);
    assert!(combined.len() <= LOG_BYTES);
    assert!(combined.contains("STDOUT_END"));
    assert!(combined.ends_with("STDERR_END"));
    assert!(!combined.contains('\u{fffd}'));
    let single = diagnostic_tail(&stdout, "");
    assert!(single.len() > LOG_BYTES / 2);
    assert!(single.len() <= LOG_BYTES);
    assert!(single.ends_with("STDOUT_END\n"));
}

#[test]
fn missing_cargo_is_reported_but_cancel_timeout_cleanup_and_reader_failures_are_preserved() {
    let command = Command::new("missing-cargo.exe");
    let result = cargo_start_failure(&command, "找不到原生工具EXE：missing-cargo.exe".into());
    assert!(result.contains("cargo --version"));
    assert!(result.contains("EDITOR_HOST_CARGO"));
    for diagnostic in [
        "构建已取消，作者文档保留",
        "构建工具探测超时，已请求终止专属进程树",
        "构建工具收口未确认，禁止采用结果或启动后续工作",
        "构建stdout读取线程异常",
        "Cargo输出读取失败：CreateProcessW 是输出内容",
    ] {
        assert_eq!(cargo_start_failure(&command, diagnostic.into()), diagnostic);
    }
}

#[test]
fn sdk_configuration_advice_identifies_the_file_without_repeating_the_configuration() {
    let original = "SDK rev 必须为完整 40 位十六进制提交 ID";
    let result = sdk_configuration_failure(Path::new("C:/Feathrise/sdk-source.toml"), original.into());
    assert!(result.starts_with(original));
    assert!(result.contains("C:/Feathrise/sdk-source.toml"));
    assert!(result.contains("不会改写已创建工程的 Cargo 文件"));
}
