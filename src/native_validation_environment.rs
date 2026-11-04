//! 自有真实作者验收的最小环境租约：隔离个人偏好与全局插件，不改变Cargo/Rustup缓存来源。

use std::{fs::{self, OpenOptions}, io::Write, path::Path, process::Command};

const PLUGIN_OWNER: &[u8] = b"feathrise-single-binary-validation-v1";

/// 调用方先核对并独占创建本轮输出根；子根只允许新建，不重用用户目录或上轮现场。
/// 只覆盖两项作者状态路径，保留现有验收开关、HOME、Cargo、Rustup及PATH，避免隐式冷编译。
pub(crate) fn isolate_author_command(command: &mut Command, output: &Path) -> Result<(), String> {
    if !output.is_absolute() { return Err("验收作者状态要求已归属的绝对输出根".into()); }
    let metadata = fs::symlink_metadata(output).map_err(|error| format!("验收输出根必须先独占创建：{error}"))?;
    #[cfg(windows)]
    let linked = { use std::os::windows::fs::MetadataExt; metadata.file_attributes() & 0x400 != 0 };
    #[cfg(not(windows))]
    let linked = metadata.file_type().is_symlink();
    if !metadata.is_dir() || linked { return Err("验收作者状态不能跟随链接或使用非目录输出根".into()); }
    let output = output.canonicalize().map_err(|error| error.to_string())?;
    let global = output.join("isolated-global-plugins");
    fs::create_dir(&global).map_err(|error| format!("隔离全局插件目录必须为本轮新建：{error}"))?;
    OpenOptions::new().write(true).create_new(true).open(global.join(".feathrise-plugin-fixture-owner"))
        .and_then(|mut file| file.write_all(PLUGIN_OWNER)).map_err(|error| error.to_string())?;
    // 维持现有验收owner对preferences/viewport相对位置的合同，文件由真实保存路径创建。
    command.env("FEATHRISE_PREFERENCES_FILE", output.join("isolated-preferences.json"))
        .env("FEATHRISE_VALIDATION_GLOBAL_PLUGINS", global);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeMap, ffi::OsString, path::PathBuf};

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("feathrise-validation-state-{}", bevy::asset::uuid::Uuid::new_v4()));
            fs::create_dir(&root).unwrap();
            Self(root.canonicalize().unwrap())
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            assert_eq!(self.0.parent(), Some(std::env::temp_dir().canonicalize().unwrap().as_path()));
            assert!(self.0.file_name().unwrap().to_string_lossy().starts_with("feathrise-validation-state-"));
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn author_state_is_fresh_and_inside_fixture_without_replacing_tool_or_user_environment() {
        let fixture = Fixture::new();
        let mut command = Command::new("not-executed-validation-child");
        for name in ["HOME", "USERPROFILE", "APPDATA", "LOCALAPPDATA", "CARGO_HOME", "RUSTUP_HOME", "PATH", "FEATHRISE_CODE_ITERATION_REPORT"] {
            command.env(name, format!("sentinel-{name}"));
        }
        let before: BTreeMap<_, _> = command.get_envs().map(|(key, value)| (key.to_owned(), value.map(ToOwned::to_owned))).collect();
        isolate_author_command(&mut command, &fixture.0).unwrap();
        let after: BTreeMap<_, _> = command.get_envs().map(|(key, value)| (key.to_owned(), value.map(ToOwned::to_owned))).collect();
        for (name, value) in before { assert_eq!(after[&name], value, "不能覆盖{name:?}"); }
        assert_eq!(after[&OsString::from("FEATHRISE_PREFERENCES_FILE")], Some(fixture.0.join("isolated-preferences.json").into_os_string()));
        let global = fixture.0.join("isolated-global-plugins");
        assert_eq!(after[&OsString::from("FEATHRISE_VALIDATION_GLOBAL_PLUGINS")], Some(global.clone().into_os_string()));
        assert_eq!(fs::read(global.join(".feathrise-plugin-fixture-owner")).unwrap(), PLUGIN_OWNER);
        assert!(!fixture.0.join("isolated-preferences.json").exists(), "不能伪造已保存偏好");
    }

    #[test]
    fn existing_state_or_invalid_root_is_not_reused_or_applied_to_command() {
        let fixture = Fixture::new();
        let global = fixture.0.join("isolated-global-plugins");
        fs::create_dir(&global).unwrap();
        fs::write(global.join("existing-user-or-old-fixture"), b"preserve").unwrap();
        let mut command = Command::new("not-executed-validation-child");
        command.env("FEATHRISE_PREFERENCES_FILE", "original");
        assert!(isolate_author_command(&mut command, &fixture.0).is_err());
        assert_eq!(fs::read(global.join("existing-user-or-old-fixture")).unwrap(), b"preserve");
        assert_eq!(command.get_envs().count(), 1);
        assert!(isolate_author_command(&mut command, Path::new("relative-output")).is_err());
        let file = fixture.0.join("not-a-directory"); fs::write(&file, b"preserve").unwrap();
        assert!(isolate_author_command(&mut command, &file).is_err());
        assert_eq!(fs::read(file).unwrap(), b"preserve");
    }
}
