//! 校正 dx fat 回执的入口捕获：官方可能任取 build.rs；只接受当前受检 scope 的精确 bin 调用。

use super::MAX_COMPILER_OUTPUT_BYTES;
use serde::Deserialize;
use serde_json::Value;
use std::{collections::BTreeSet, fs::OpenOptions, io::Read, os::windows::fs::OpenOptionsExt, path::{Path, PathBuf}};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Invocation { args: Vec<String>, envs: Vec<(String, String)> }

/// 只替换同一次完整构建的 rustc 参数及环境；fat EXE、原始 link_args 和资产回执保持原值。
pub(super) fn normalize(client: &mut Value, target: &Path, bin: &str, package_manifest: &Path,
    package: &str, link_context: &Path) -> Result<(), String> {
    let tip = bin.replace('-', "_");
    let original = Invocation {
        args: serde_json::from_value(client["rustc_args"].clone()).map_err(|error| error.to_string())?,
        envs: serde_json::from_value(client["rustc_envs"].clone()).map_err(|error| error.to_string())?,
    };
    validate_strings(&original)?;
    validate_project(&original, target, package_manifest, package)?;
    let scope = capture_scope(&original, target, &tip)?;
    let mut selected = if argument(&original.args, "--crate-name")? == tip { original } else {
        // 官方输出从 HashMap 任取首个 .bin；旧 build.rs 捕获可复用，但不能拿它重编预览入口。
        // 该旧记录仅提供受限 scope 定位，最终入口捕获必须绑定当前独占 link-context，拒绝旧会话。
        let path = scope.join(format!("{tip}.bin.json"));
        let canonical = path.canonicalize().map_err(|error| format!("缺少本次精确入口捕获：{error}"))?;
        if canonical != path { return Err("入口捕获文件重定向到其他路径".into()); }
        let mut bytes = Vec::new();
        OpenOptions::new().read(true).share_mode(1).open(&canonical).map_err(|error| error.to_string())?
            .take(MAX_COMPILER_OUTPUT_BYTES as u64 + 1).read_to_end(&mut bytes).map_err(|error| error.to_string())?;
        if bytes.len() > MAX_COMPILER_OUTPUT_BYTES { return Err("入口捕获超过8MiB".into()); }
        serde_json::from_slice::<Invocation>(&bytes).map_err(|error| format!("入口捕获无效：{error}"))?
    };
    validate_strings(&selected)?;
    validate_project(&selected, target, package_manifest, package)?;
    if argument(&selected.args, "--crate-name")? != tip || argument(&selected.args, "--crate-type")? != "bin"
        || argument(&selected.args, "--target")? != "x86_64-pc-windows-msvc"
        || environment(&selected, "CARGO_BIN_NAME")? != bin
        || capture_scope(&selected, target, &tip)? != scope
        || environment_path(&selected, "FEATHRISE_HOTPATCH_LINK_CONTEXT")? != link_context {
        return Err("入口捕获的bin、target或链接会话不属于本次fat构建".into());
    }
    // Cargo jobserver 句柄只在本次 Cargo 调度期间有效；冻结后由独立 thin rustc 重放，
    // 不能沿用已退出 Cargo 的 semaphore。仅清理这两个临时调度键，保留业务环境与入口身份。
    selected.envs.retain(|(name, _)| !matches!(name.as_str(), "CARGO_MAKEFLAGS" | "MAKEFLAGS"));
    // 所有身份门通过后才一次性更换参数，失败不留下半份修正后的 client。
    let args = serde_json::to_value(selected.args).map_err(|error| error.to_string())?;
    let envs = serde_json::to_value(selected.envs).map_err(|error| error.to_string())?;
    client["rustc_args"] = args; client["rustc_envs"] = envs;
    Ok(())
}

fn validate_strings(invocation: &Invocation) -> Result<(), String> {
    let mut names = BTreeSet::new();
    if invocation.args.iter().any(|value| value.contains('\0')) || invocation.envs.iter().any(|(name, value)|
        name.is_empty() || name.contains('\0') || value.contains('\0') || !names.insert(name)) {
        return Err("入口捕获包含NUL、空环境名称或重复环境键".into());
    }
    Ok(())
}

fn validate_project(invocation: &Invocation, target: &Path, package_manifest: &Path, package: &str) -> Result<(), String> {
    // editor-project根可包围虚拟workspace和叶包；Cargo的MANIFEST_DIR指实际选中包，
    // 必须按该目录里的精确Cargo.toml核验，完整工程root/id/manifest仍由冻结Context另行绑定。
    let manifest = environment_path(invocation, "CARGO_MANIFEST_DIR")?.join("Cargo.toml")
        .canonicalize().map_err(|error| format!("入口捕获Cargo清单不可读：{error}"))?;
    if environment_path(invocation, "CARGO_TARGET_DIR")? != target
        || manifest != package_manifest
        || environment(invocation, "CARGO_PKG_NAME")? != package {
        return Err("入口捕获的Cargo清单、包或target与本次fat构建不同".into());
    }
    Ok(())
}

fn capture_scope(invocation: &Invocation, target: &Path, tip: &str) -> Result<PathBuf, String> {
    let scope = environment_path(invocation, "DX_RUSTC")?;
    let prefix = format!("{tip}-x86_64-pc-windows-msvc-render-dev-");
    let suffix = scope.file_name().and_then(|name| name.to_str()).and_then(|name| name.strip_prefix(prefix.as_str()));
    if scope.parent() != Some(target.join("dx/.captured-args").as_path())
        || !scope.is_dir() || !suffix.is_some_and(|value| value.len() == 16
            && value.bytes().all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))) {
        return Err("入口捕获scope不属于本次target/bin/开发配置".into());
    }
    Ok(scope)
}

fn argument<'a>(arguments: &'a [String], name: &str) -> Result<&'a str, String> {
    let mut found = None;
    for (index, value) in arguments.iter().enumerate() {
        let next = if value == name { Some(arguments.get(index + 1).ok_or("入口捕获参数缺少值")?.as_str()) }
            else { value.strip_prefix(name).and_then(|value| value.strip_prefix('=')) };
        if let Some(value) = next && found.replace(value).is_some() { return Err("入口捕获参数重复".into()); }
    }
    found.ok_or_else(|| format!("入口捕获缺少{name}"))
}

fn environment<'a>(invocation: &'a Invocation, name: &str) -> Result<&'a str, String> {
    invocation.envs.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str())
        .ok_or_else(|| format!("入口捕获缺少{name}"))
}

fn environment_path(invocation: &Invocation, name: &str) -> Result<PathBuf, String> {
    Path::new(environment(invocation, name)?).canonicalize().map_err(|error| format!("入口捕获{name}路径无效：{error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    struct Fixture { directory: PathBuf, target: PathBuf, project: PathBuf, scope: PathBuf, link: PathBuf }
    impl Fixture {
        fn new() -> Self {
            let directory = std::env::temp_dir().join(format!("feathrise-dx-client-{}", bevy::asset::uuid::Uuid::new_v4()));
            fs::create_dir_all(&directory).unwrap(); let directory = directory.canonicalize().unwrap();
            let target = directory.join("target"); let project = directory.join("中文工程");
            let scope = target.join("dx/.captured-args/actual_bin-x86_64-pc-windows-msvc-render-dev-0123456789abcdef");
            let link = target.join("hotpatch-tools/function-sessions/current/link-context.txt");
            for path in [&scope, &project, link.parent().unwrap()] { fs::create_dir_all(path).unwrap(); }
            fs::write(project.join("Cargo.toml"), "[package]\nname='different-package'\nversion='0.1.0'\n").unwrap();
            fs::write(&link, b"current-session").unwrap();
            Self { directory, target, project, scope, link }
        }
        fn invocation(&self, script: bool) -> Value {
            let mut envs = json!([["DX_RUSTC", self.scope], ["CARGO_TARGET_DIR", self.target],
                ["CARGO_MANIFEST_DIR", self.project], ["CARGO_PKG_NAME", "different-package"],
                ["FEATHRISE_HOTPATCH_LINK_CONTEXT", self.link]]);
            let args = if script { json!(["rustc", "--crate-name", "build_script_build", "--crate-type", "bin", "build.rs"]) }
                else { envs.as_array_mut().unwrap().push(json!(["CARGO_BIN_NAME", "actual-bin"]));
                    json!(["rustc", "--crate-name", "actual_bin", "--crate-type", "bin", "--target", "x86_64-pc-windows-msvc", "src/main.rs"]) };
            json!({"args":args,"envs":envs})
        }
        fn client(&self, script: bool) -> Value {
            let invocation = self.invocation(script);
            json!({"rustc_args":invocation["args"],"rustc_envs":invocation["envs"],
                "exe":self.target.join("original-fat.exe"),"link_args":["frozen-actual-import.dll.lib"],"assets":[],"path":self.target})
        }
        fn normalize(&self, client: &mut Value) -> Result<(), String> {
            normalize(client, &self.target, "actual-bin", &self.project.join("Cargo.toml"), "different-package", &self.link)
        }
    }
    impl Drop for Fixture { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.directory); } }

    #[test]
    fn nested_leaf_manifest_uses_the_selected_package_and_rejects_foreign_capture_identity() {
        let fixture = Fixture::new();
        let leaf = fixture.project.join("game"); fs::create_dir(&leaf).unwrap();
        let manifest = leaf.join("Cargo.toml");
        fs::write(&manifest,"[package]\nname='different-package'\nversion='0.1.0'\n").unwrap();
        fs::write(fixture.project.join("Cargo.toml"),"[workspace]\nmembers=['game']\n").unwrap();
        let set_env = |client: &mut Value, name: &str, value: Value| {
            client["rustc_envs"].as_array_mut().unwrap().iter_mut().find(|pair| pair[0] == name).unwrap()[1] = value;
        };
        let mut client = fixture.client(false); set_env(&mut client,"CARGO_MANIFEST_DIR",json!(leaf));
        let before = client.clone();
        normalize(&mut client,&fixture.target,"actual-bin",&manifest,"different-package",&fixture.link).unwrap();
        assert_eq!(client,before,"只修正包根身份，不能改写fat回执或工程根");
        let other_target = fixture.directory.join("other-target"); fs::create_dir(&other_target).unwrap();
        for (name,value) in [
            ("CARGO_MANIFEST_DIR",json!(fixture.project)),
            ("CARGO_TARGET_DIR",json!(other_target)),
            ("CARGO_PKG_NAME",json!("other-package")),
        ] {
            let mut wrong = before.clone(); set_env(&mut wrong,name,value); let unchanged = wrong.clone();
            assert!(normalize(&mut wrong,&fixture.target,"actual-bin",&manifest,"different-package",&fixture.link).is_err());
            assert_eq!(wrong,unchanged,"错误身份拒绝时不能留下半份client");
        }
        let mut wrong = before.clone();
        assert!(normalize(&mut wrong,&fixture.target,"actual-bin",&fixture.project.join("Cargo.toml"),"different-package",&fixture.link).is_err(),
            "虚拟workspace清单不能代替实际叶包清单");
        assert_eq!(wrong,before);
    }

    #[test]
    fn wrong_build_script_client_uses_exact_current_tip_and_preserves_fat_receipt() {
        let fixture = Fixture::new(); let selected = fixture.invocation(false);
        fs::write(fixture.scope.join("actual_bin.bin.json"), selected.to_string()).unwrap();
        fs::write(fixture.scope.join("another.bin.json"), fixture.invocation(true).to_string()).unwrap();
        let mut client = fixture.client(true); let before = client.clone();
        // build.rs可以来自旧会话；scope只作定位，最终tip必须来自当前链接会话。
        client["rustc_envs"].as_array_mut().unwrap().iter_mut().find(|pair| pair[0] == "FEATHRISE_HOTPATCH_LINK_CONTEXT")
            .unwrap()[1] = json!(fixture.directory.join("old-link-context.txt"));
        fixture.normalize(&mut client).unwrap();
        assert_eq!(client["rustc_args"], selected["args"]); assert_eq!(client["rustc_envs"], selected["envs"]);
        for key in ["exe", "link_args", "assets", "path"] { assert_eq!(client[key], before[key]); }
    }

    #[test]
    fn matching_tip_needs_no_capture_file_but_rejects_old_link_session() {
        let fixture = Fixture::new(); let mut client = fixture.client(false);
        fixture.normalize(&mut client).unwrap();
        let old = fixture.directory.join("old-link-context.txt"); fs::write(&old, b"old").unwrap();
        client["rustc_envs"].as_array_mut().unwrap().iter_mut().find(|pair| pair[0] == "FEATHRISE_HOTPATCH_LINK_CONTEXT")
            .unwrap()[1] = json!(old);
        assert!(fixture.normalize(&mut client).is_err());
    }

    #[test]
    fn frozen_tip_removes_only_cargo_jobserver_keys_and_keeps_business_environment() {
        let fixture = Fixture::new(); let mut clean = fixture.invocation(false);
        clean["envs"].as_array_mut().unwrap().extend([
            json!(["PROJECT_GAMEPLAY_MODE", "validation"]), json!(["CARGO_BUILD_JOBS", "6"]),
        ]);
        let mut scheduled = clean.clone();
        scheduled["envs"].as_array_mut().unwrap().extend([
            json!(["CARGO_MAKEFLAGS", "--jobserver-auth=expired-cargo-semaphore"]),
            json!(["MAKEFLAGS", "-j --jobserver-auth=expired-cargo-semaphore"]),
        ]);
        fs::write(fixture.scope.join("actual_bin.bin.json"), scheduled.to_string()).unwrap();
        for script in [false, true] {
            let mut client = fixture.client(script); let mut expected = client.clone();
            expected["rustc_args"] = clean["args"].clone(); expected["rustc_envs"] = clean["envs"].clone();
            if !script { client["rustc_envs"] = scheduled["envs"].clone(); }
            fixture.normalize(&mut client).unwrap();
            assert_eq!(client, expected);
        }
    }

    #[test]
    fn missing_wrong_scope_or_stale_tip_never_uses_another_capture() {
        let fixture = Fixture::new(); let mut client = fixture.client(true); let before = client.clone();
        fs::write(fixture.scope.join("another.bin.json"), fixture.invocation(false).to_string()).unwrap();
        assert!(fixture.normalize(&mut client).is_err()); assert_eq!(client, before);
        let mut selected = fixture.invocation(false);
        selected["envs"].as_array_mut().unwrap().iter_mut().find(|pair| pair[0] == "CARGO_BIN_NAME")
            .unwrap()[1] = json!("another-bin");
        fs::write(fixture.scope.join("actual_bin.bin.json"), selected.to_string()).unwrap();
        assert!(fixture.normalize(&mut client).is_err()); assert_eq!(client, before);
        client["rustc_envs"].as_array_mut().unwrap().iter_mut().find(|pair| pair[0] == "DX_RUSTC")
            .unwrap()[1] = json!(fixture.directory);
        assert!(fixture.normalize(&mut client).is_err());
    }
}
