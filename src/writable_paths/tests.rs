//! 可写路径策略的纯函数回归；不修改进程环境、不创建新构建缓存、不依赖安装目录可写。

use super::*;

fn root(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("feathrise-writable-path-test-{name}"))
}

#[test]
fn installed_cache_is_user_owned_even_when_resources_have_an_old_target() {
    let resources = root("read-only-install");
    let local = root("user-local");
    let target = default_target(ResourceMode::Installed, &resources, Some(&local), None).unwrap();
    assert_eq!(target, local.join("Feathrise/target"));
    assert!(!target.starts_with(&resources));
    // 选择过程不查询/搬迁旧 target；旧目录是否存在不影响安装模式写入归属。
    assert_ne!(target, resources.join("target"));
}

#[test]
fn source_development_keeps_existing_target_without_requiring_user_cache_environment() {
    let source = root("source");
    assert_eq!(
        default_target(ResourceMode::Development, &source, Some(Path::new("invalid-local")), None).unwrap(),
        source.join("target"),
    );
}

#[test]
fn command_line_wins_then_cargo_environment_and_relative_override_is_frozen_once() {
    let command_line = root("explicit-cache");
    let cargo = root("cargo-cache");
    assert_eq!(selected_override(Some(&command_line), Some(&cargo)), Some((command_line.as_path(), "--target-dir")));
    assert_eq!(selected_override(None, Some(&cargo)), Some((cargo.as_path(), "CARGO_TARGET_DIR")));
    assert_eq!(selected_override(None, None), None);
    let startup = root("startup-cwd");
    let chosen = absolute_target(Path::new("existing-cache"), &startup, "CARGO_TARGET_DIR").unwrap();
    assert_eq!(chosen, startup.join("existing-cache"));
    // 作者实例收到绝对 --target-dir，切到工程目录也不能二次解释它。
    assert_eq!(absolute_target(&chosen, &root("project-cwd"), "--target-dir").unwrap(), chosen);
    assert_eq!(absolute_target(&command_line, Path::new(""), "--target-dir").unwrap(), command_line);
    assert_eq!(project_target(&root("project"), Some(&chosen)).unwrap(), chosen);
    let parent_segments = startup.join("old/../existing-cache");
    assert_eq!(project_target(&root("project"), Some(&parent_segments)).unwrap(), parent_segments);
}

#[test]
fn missing_local_data_uses_platform_user_home_and_invalid_explicit_value_is_not_ignored() {
    let home = root("user-home");
    let expected = if cfg!(windows) { home.join("AppData/Local/Feathrise/target") } else { home.join(".cache/Feathrise/target") };
    assert_eq!(default_target(ResourceMode::Installed, Path::new("unused"), None, Some(&home)).unwrap(), expected);
    for invalid in [Path::new(""), Path::new("relative"), Path::new("../outside")] {
        let error = default_target(ResourceMode::Installed, Path::new("unused"), Some(invalid), Some(&home)).unwrap_err();
        assert!(error.contains(if cfg!(windows) { "LOCALAPPDATA" } else { "XDG_CACHE_HOME" }));
    }
    assert!(default_target(ResourceMode::Installed, Path::new("unused"), None, None).unwrap_err().contains("--target-dir"));
}

#[test]
fn personal_preferences_share_user_root_with_global_plugins_and_keep_legacy_lookup_separate() {
    let home = root("home");
    let roaming = root("roaming");
    let file = preferences_file(&home).unwrap();
    assert_eq!(file, home.join(".feathrise/editor.json"));
    assert_eq!(file.parent().unwrap().join("plugins"), home.join(".feathrise/plugins"));
    assert_eq!(file.with_extension("author.json"), home.join(".feathrise/editor.author.json"));
    assert_eq!(legacy_preferences_file_at(&home, Some(&roaming)), roaming.join("Feathrise/editor.json"));
    assert_eq!(legacy_preferences_file_at(&home, None), home.join(".config/feathrise/editor.json"));
    assert!(preferences_file(Path::new("relative")).is_err());
}

#[test]
fn empty_cache_override_is_diagnosed_instead_of_silently_selecting_a_different_cache() {
    let cwd = root("cwd");
    for label in ["--target-dir", "CARGO_TARGET_DIR"] {
        let error = absolute_target(Path::new(""), &cwd, label).unwrap_err();
        assert!(error.contains(label));
        assert!(error.contains("不能为空"));
    }
    #[cfg(windows)]
    for relative_drive in ["C:cache", r"\cache"] {
        assert!(absolute_target(Path::new(relative_drive), &cwd, "--target-dir").is_err());
    }
}
