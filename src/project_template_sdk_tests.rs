//! SDK 来源回归：配置边界、同源补丁、旧工程只读策略以及 Cargo 真实本地 Git 解析。

use super::*;
use std::fs;

const MINIMAL_TEMPLATE: &str = "[package]\nname='fixture_game'\nversion='0.1.0'\nedition='2024'\n[[bin]]\nname='fixture_game'\npath='src/main.rs'\n[dependencies]\nfeathrise_runtime={path='ignored',default-features=false}\n";

struct Fixture { root: PathBuf, editor: PathBuf, sdk: PathBuf }

impl Fixture {
    fn new() -> Self {
        let parent = std::env::temp_dir().canonicalize().unwrap();
        let root = parent.join(format!("feathrise-sdk-{}", super::super::new_uuid().unwrap()));
        fs::create_dir(&root).unwrap();
        let editor = root.join("编辑器 空格");
        let sdk = root.join("feathrise_runtime");
        fs::create_dir(&editor).unwrap();
        let fixture = Self { root, editor, sdk };
        fixture.write_sdk();
        fixture
    }

    fn write_sdk(&self) {
        fs::create_dir_all(self.sdk.join("src")).unwrap();
        let mut manifest = "[package]\nname='feathrise_runtime'\nversion='0.1.0'\nedition='2024'\n[workspace]\nresolver='3'\nmembers=['third_party/*']\n[dependencies]\n".to_owned();
        for (package, directory) in PATCHES {
            let root = self.sdk.join("third_party").join(directory);
            fs::create_dir_all(root.join("src")).unwrap();
            fs::write(root.join("src/lib.rs"), "// 中性来源测试夹具。\n").unwrap();
            fs::write(root.join("Cargo.toml"), format!("[package]\nname='{package}'\nversion='0.1.0'\nedition='2024'\n")).unwrap();
            // 全部使用仓内 path，fixture 没有 registry 依赖，也没有测试用 target/profile 改写。
            manifest.push_str(&format!("{package}={{path='third_party/{directory}'}}\n"));
        }
        fs::write(self.sdk.join("Cargo.toml"), manifest).unwrap();
        fs::write(self.sdk.join("src/lib.rs"), "// 中性 SDK 入口夹具。\n").unwrap();
    }

    fn config(&self, source: &str) {
        fs::write(self.editor.join(CONFIG_NAME), format!("format=1\n[source]\n{source}\n")).unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // UUID 夹具不保存作者文件；递归清理前再次确认仍位于本次创建的临时根。
        let actual = self.root.canonicalize().unwrap();
        assert_eq!(actual, self.root);
        assert_eq!(actual.parent(), Some(std::env::temp_dir().canonicalize().unwrap().as_path()));
        assert!(actual.file_name().unwrap().to_string_lossy().starts_with("feathrise-sdk-"));
        fs::remove_dir_all(actual).unwrap();
    }
}

fn generated(source: &SdkSource) -> Value {
    toml::from_str(&source.apply_template(MINIMAL_TEMPLATE).unwrap()).unwrap()
}

#[test]
fn missing_config_keeps_legacy_layout_and_explicit_local_paths_are_not_cwd_relative() {
    let fixture = Fixture::new();
    for configured in [false, true] {
        if configured { fixture.config("kind='path'\npath='../feathrise_runtime'"); }
        let value = generated(&SdkSource::load(&fixture.editor).unwrap());
        assert_eq!(value["dependencies"]["feathrise_runtime"]["path"].as_str(), Some(super::super::toml_path(&fixture.sdk).unwrap().as_str()));
        assert_eq!(value["bin"][0]["path"].as_str(), Some("src/main.rs"));
        assert_eq!(value["dependencies"]["feathrise_runtime"]["default-features"].as_bool(), Some(false));
        for (package, directory) in PATCHES {
            let expected = super::super::toml_path(&fixture.sdk.join("third_party").join(directory)).unwrap();
            assert_eq!(value["patch"]["crates-io"][package]["path"].as_str(), Some(expected.as_str()));
        }
    }
    let renamed = fixture.root.join("本地 SDK 带空格");
    fs::rename(&fixture.sdk, &renamed).unwrap();
    fixture.config("kind='path'\npath='../本地 SDK 带空格'");
    assert!(SdkSource::load(&fixture.editor).is_ok());
    // 原路径仍存在与否不控制显式配置，绝对 Unicode 路径也按 TOML 正确编码。
    let path = super::super::toml_path(&renamed).unwrap();
    fixture.config(&format!("kind='path'\npath={}", Value::String(path)));
    assert!(SdkSource::load(&fixture.editor).is_ok());
}

#[test]
fn cached_templates_keep_relative_sdk_paths_bound_to_external_configuration() {
    let fixture = Fixture::new();
    fixture.config("kind='path'\npath='../feathrise_runtime'");
    let cached = fixture.root.join("缓存/资源身份");
    let template = cached.join("assets/templates/empty");
    fs::create_dir_all(template.join("src")).unwrap();
    let source = "[package]\nname = \"feathrise_game_template\"\nversion = \"0.1.0\"\nedition = \"2024\"\ndefault-run = \"game\"\n[[bin]]\nname = \"game\"\npath = \"src/main.rs\"\n[dependencies]\nfeathrise_runtime = { path = \"ignored\", default-features = false }\n";
    fs::write(template.join("Cargo.toml"), source).unwrap();
    fs::write(template.join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(template.join("build.rs"), "fn main() {}\n").unwrap();
    fs::write(template.join("README.md"), "缓存中的中性模板\n").unwrap();
    fs::write(cached.join("Cargo.lock"), "version = 4\n").unwrap();
    // 缓存中的同名配置不是来源入口；安装目录则只有 SDK 配置，没有模板和种子锁。
    fs::write(cached.join(CONFIG_NAME), "不是有效的 SDK 配置").unwrap();
    assert!(!fixture.editor.join("templates").exists());
    assert!(!fixture.editor.join("Cargo.lock").exists());
    let configuration_before = fs::read(fixture.editor.join(CONFIG_NAME)).unwrap();
    let project = fixture.root.join("生成缓存工程");
    super::super::empty::write_with_sdk_configuration(
        &project,
        "缓存模板工程",
        &super::super::new_uuid().unwrap(),
        &cached,
        &fixture.editor,
        &crate::build_process::BuildControl::default(),
        feathrise_runtime::GameView::ThirdPerson,
    ).unwrap();

    let manifest: Value = toml::from_str(&fs::read_to_string(project.join("Cargo.toml")).unwrap()).unwrap();
    let expected = super::super::toml_path(&fixture.sdk).unwrap();
    assert_eq!(manifest["dependencies"]["feathrise_runtime"]["path"].as_str(), Some(expected.as_str()));
    for (package, directory) in PATCHES {
        let expected = super::super::toml_path(&fixture.sdk.join("third_party").join(directory)).unwrap();
        assert_eq!(manifest["patch"]["crates-io"][package]["path"].as_str(), Some(expected.as_str()));
    }
    assert_eq!(manifest["profile"]["dev"]["opt-level"].as_integer(), Some(0));
    assert_eq!(manifest["profile"]["dev"]["package"]["*"]["opt-level"].as_integer(), Some(3));
    assert_eq!(manifest["profile"]["dev"]["package"].as_table().unwrap().len(), 1);
    assert_eq!(manifest["profile"]["render-dev"]["inherits"].as_str(), Some("dev"));
    assert_eq!(manifest["profile"]["render-dev"].as_table().unwrap().len(), 1);
    assert_eq!(fs::read_to_string(project.join("README.md")).unwrap(), "缓存中的中性模板\n");
    assert!(project.join("Cargo.lock").is_file());
    assert_eq!(fs::read(fixture.editor.join(CONFIG_NAME)).unwrap(), configuration_before);
    assert_eq!(fs::read_to_string(cached.join("Cargo.lock")).unwrap(), "version = 4\n");
    assert_eq!(fs::read_to_string(template.join("Cargo.toml")).unwrap(), source);
}

#[test]
fn malformed_or_incomplete_source_never_silently_falls_back() {
    let fixture = Fixture::new();
    for source in [
        "kind='path'\npath='missing'",
        "kind='path'\npath=''",
        "kind='path'\npath='../feathrise_runtime'\nrev='ignored'",
        "kind='git'\ngit='https://example.invalid/sdk.git'\nrev='main'",
        "kind='git'\ngit='https://example.invalid/sdk.git'\nrev='abc123'",
        "kind='git'\ngit='https://example.invalid/sdk.git'\nrev='000000000000000000000000000000000000000g'",
        "kind='git'\ngit='https://example.invalid/sdk.git'\nrev='0000000000000000000000000000000000000000'\nbranch='main'",
        "kind='git'\ngit='local/path'\nrev='0000000000000000000000000000000000000000'",
        "kind='git'\ngit='https://example.invalid/sdk.git#main'\nrev='0000000000000000000000000000000000000000'",
    ] {
        fixture.config(source);
        let before = fs::read(fixture.editor.join(CONFIG_NAME)).unwrap();
        let error = SdkSource::load(&fixture.editor).err().expect("错误配置必须拒绝");
        assert!(error.contains("SDK 配置位置："), "{error}");
        assert!(error.contains("不会改写已创建工程的 Cargo 文件"));
        assert_eq!(fs::read(fixture.editor.join(CONFIG_NAME)).unwrap(), before);
    }
    fs::write(fixture.editor.join(CONFIG_NAME), "format=2\n[source]\nkind='path'\npath='../feathrise_runtime'").unwrap();
    assert!(SdkSource::load(&fixture.editor).err().unwrap().contains("版本"));
    fs::write(fixture.editor.join(CONFIG_NAME), " ".repeat(CONFIG_LIMIT + 1)).unwrap();
    assert!(SdkSource::load(&fixture.editor).err().unwrap().contains("16 KiB"));
    fixture.config("kind='path'\npath='../feathrise_runtime'");
    fs::write(fixture.sdk.join("third_party/parley-compat/Cargo.toml"), "[package]\nname='other-package'\n").unwrap();
    assert!(SdkSource::load(&fixture.editor).err().unwrap().contains("包身份"));
}

#[test]
fn git_source_has_one_revision_for_sdk_and_all_patches_without_sibling_requirement() {
    let fixture = Fixture::new();
    let revision = "ABCDEF0123".repeat(4);
    fixture.config(&format!("kind='git'\ngit='https://example.invalid/sdk.git'\nrev='{revision}'"));
    fs::rename(&fixture.sdk, fixture.root.join("unused-sdk")).unwrap();
    let source = SdkSource::load(&fixture.editor).unwrap();
    let value = generated(&source);
    let sdk = &value["dependencies"]["feathrise_runtime"];
    assert_eq!(sdk["rev"].as_str(), Some(revision.to_ascii_lowercase().as_str()));
    assert!(sdk.get("path").is_none());
    for (package, _) in PATCHES {
        let patch = &value["patch"]["crates-io"][package];
        assert_eq!(patch["git"], sdk["git"]);
        assert_eq!(patch["rev"], sdk["rev"]);
        assert!(patch.get("path").is_none());
    }
    let mut command = Command::new("cargo");
    source.apply_network_policy(&mut command);
    assert!(!command.get_args().any(|arg| arg == "--offline"));
}

#[test]
fn separate_patch_checkout_is_used_for_path_and_git_sources() {
    let fixture = Fixture::new();
    let patches = fixture.root.join("单独补丁仓");
    fs::rename(fixture.sdk.join("third_party"), &patches).unwrap();
    fixture.config("kind='path'\npath='../feathrise_runtime'\npatch_path='../单独补丁仓'");
    let manifest = generated(&SdkSource::load(&fixture.editor).unwrap());
    for (package, directory) in PATCHES {
        let path = super::super::toml_path(&patches.join(directory)).unwrap();
        assert_eq!(manifest["patch"]["crates-io"][package]["path"].as_str(), Some(path.as_str()));
    }

    let sdk_rev = "a".repeat(40);
    let patch_rev = "b".repeat(40);
    let git = format!("kind='git'\ngit='https://example.invalid/sdk.git'\nrev='{sdk_rev}'\npatch_git='https://example.invalid/patches.git'");
    fixture.config(&git);
    assert!(SdkSource::load(&fixture.editor).is_err());
    fixture.config(&format!("{git}\npatch_rev='{patch_rev}'"));
    let manifest = generated(&SdkSource::load(&fixture.editor).unwrap());
    assert_eq!(manifest["dependencies"]["feathrise_runtime"]["rev"].as_str(), Some(sdk_rev.as_str()));
    for (package, _) in PATCHES {
        let patch = &manifest["patch"]["crates-io"][package];
        assert_eq!(patch["git"].as_str(), Some("https://example.invalid/patches.git"));
        assert_eq!(patch["rev"].as_str(), Some(patch_rev.as_str()));
    }
}

#[test]
fn registry_source_pins_sdk_version_and_git_patches_without_a_sibling_checkout() {
    let fixture = Fixture::new();
    let revision = "ABCDEF0123".repeat(4);
    fixture.config(&format!("kind='registry'\nversion='0.1.0-beta.1'\ngit='https://example.invalid/sdk.git'\nrev='{revision}'"));
    fs::rename(&fixture.sdk, fixture.root.join("unused-sdk")).unwrap();
    let source = SdkSource::load(&fixture.editor).unwrap();
    let value = generated(&source);
    let sdk = &value["dependencies"]["feathrise_runtime"];
    assert_eq!(sdk["version"].as_str(), Some("=0.1.0-beta.1"));
    assert_eq!(sdk["default-features"].as_bool(), Some(false));
    for key in ["path", "git", "rev", "registry"] {
        assert!(sdk.get(key).is_none(), "crates.io SDK 不携带本机路径或 Git 来源");
    }
    assert_eq!(value["patch"]["crates-io"].as_table().unwrap().len(), PATCHES.len());
    for (package, _) in PATCHES {
        let patch = &value["patch"]["crates-io"][package];
        assert_eq!(patch["git"].as_str(), Some("https://example.invalid/sdk.git"));
        assert_eq!(patch["rev"].as_str(), Some(revision.to_ascii_lowercase().as_str()));
        assert!(patch.get("path").is_none());
        assert!(patch.get("version").is_none());
    }
    let mut command = Command::new("cargo");
    source.apply_network_policy(&mut command);
    assert!(!command.get_args().any(|arg| arg == "--offline"));
}

#[test]
fn registry_configuration_rejects_version_ranges_and_unpinned_patch_sources() {
    let fixture = Fixture::new();
    let revision = "a".repeat(40);
    for version in ["", "main", "*", "^0.1.0", "0.1", " 0.1.0"] {
        fixture.config(&format!("kind='registry'\nversion='{version}'\ngit='https://example.invalid/sdk.git'\nrev='{revision}'"));
        let before = fs::read(fixture.editor.join(CONFIG_NAME)).unwrap();
        assert!(SdkSource::load(&fixture.editor).is_err(), "{version}");
        assert_eq!(fs::read(fixture.editor.join(CONFIG_NAME)).unwrap(), before);
    }
    for extra in ["rev='main'", "rev='abc123'", "rev='000000000000000000000000000000000000000g'", "rev='aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'\npath='../feathrise_runtime'"] {
        fixture.config(&format!("kind='registry'\nversion='0.1.0'\ngit='https://example.invalid/sdk.git'\n{extra}"));
        assert!(SdkSource::load(&fixture.editor).is_err(), "{extra}");
    }
}

#[test]
fn existing_project_network_policy_is_read_only_and_does_not_assume_new_source_config() {
    let fixture = Fixture::new();
    // 即使当前新建配置已经坏掉或改变，F12 仍只解释旧工程自己的 manifest。
    fs::write(fixture.editor.join(CONFIG_NAME), "broken new configuration").unwrap();
    let manifest = fixture.root.join("Cargo.toml");
    for (source, offline) in [
        ("[dependencies]\nfeathrise_runtime={path='current-sdk'}", true),
        ("[dependencies]\nmy_alias={package='feathrise_runtime',path='current-sdk'}", true),
        ("[dependencies]\nbevy_feathrise={path='old-sdk'}", true),
        ("[dependencies]\nnew_editor_game_runtime={path='old-sdk'}", true),
        ("[dependencies]\nmy_alias={package='bevy_feathrise',path='old-sdk'}", true),
        ("[dependencies]\nbevy_feathrise={git='https://example.invalid/old.git',rev='old-author-rev'}", false),
        ("[dependencies]\nbevy_feathrise={workspace=true}", false),
        ("[dependencies]\nbevy_feathrise={version='=0.1.0'}", false),
        ("[dependencies]\nbevy_feathrise={version='=0.1.0'}\n[patch.crates-io]\nbevy_render={git='https://example.invalid/sdk.git',rev='old-author-rev'}", false),
        ("[dependencies]\nbevy_feathrise={path='old-sdk'}\ncustom={git='https://example.invalid/custom.git'}", false),
        ("[dependencies]\nbevy_feathrise={path='old-sdk'}\ncustom={workspace=true}", false),
        ("[dependencies]\ncustom_engine={path='old-sdk'}", false),
    ] {
        fs::write(&manifest, source).unwrap();
        let mut command = Command::new("cargo");
        apply_project_network_policy(&manifest, &mut command).unwrap();
        assert_eq!(command.get_args().any(|arg| arg == "--offline"), offline, "{source}");
        assert_eq!(fs::read_to_string(&manifest).unwrap(), source);
    }
    fs::write(&manifest, " ".repeat(MANIFEST_LIMIT + 1)).unwrap();
    assert!(apply_project_network_policy(&manifest, &mut Command::new("cargo")).unwrap_err().contains("64 KiB"));
}

#[test]
fn template_source_application_preserves_code_paths_and_rejects_duplicate_patch_authority() {
    let fixture = Fixture::new();
    let source = SdkSource::load(&fixture.editor).unwrap();
    let custom = MINIMAL_TEMPLATE.replace("src/main.rs", "../shared/main.rs");
    let generated: Value = toml::from_str(&source.apply_template(&custom).unwrap()).unwrap();
    assert_eq!(generated["bin"][0]["path"].as_str(), Some("../shared/main.rs"));
    for table in ["[workspace]", "[patch.crates-io]"] {
        assert!(source.apply_template(&format!("{custom}\n{table}\n")).is_err());
    }
}

fn checked_output(command: &mut Command) -> std::process::Output {
    let result = command.output().unwrap();
    assert!(result.status.success(), "{:?}\n{}\n{}", command, String::from_utf8_lossy(&result.stdout), String::from_utf8_lossy(&result.stderr));
    result
}

fn file_url(path: &Path) -> String {
    let normalized = super::super::toml_path(path).unwrap();
    let mut encoded = String::from("file:///");
    for byte in normalized.bytes() {
        if byte.is_ascii_alphanumeric() || b"/:.-_~".contains(&byte) {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

#[test]
fn local_git_cargo_metadata_resolves_sdk_and_eight_patches_from_the_exact_commit() {
    let fixture = Fixture::new();
    checked_output(Command::new("git").args(["init", "--quiet"]).arg(&fixture.sdk));
    checked_output(Command::new("git").arg("-C").arg(&fixture.sdk).args(["add", "."]));
    checked_output(Command::new("git").arg("-C").arg(&fixture.sdk).args([
        "-c", "user.name=Feathrise fixture", "-c", "user.email=fixture@example.invalid",
        "-c", "commit.gpgsign=false", "commit", "--quiet", "-m", "中性 SDK 来源夹具",
    ]));
    let revision = checked_output(Command::new("git").arg("-C").arg(&fixture.sdk).args(["rev-parse", "HEAD"]));
    let revision = String::from_utf8(revision.stdout).unwrap().trim().to_owned();
    fixture.config(&format!("kind='git'\ngit='{}'\nrev='{revision}'", file_url(&fixture.sdk)));
    let source = SdkSource::load(&fixture.editor).unwrap();
    let project = fixture.root.join("生成 工程");
    fs::create_dir_all(project.join("src")).unwrap();
    fs::write(project.join("src/main.rs"), "fn main() {}\n").unwrap();
    let manifest = source.apply_template(MINIMAL_TEMPLATE).unwrap();
    fs::write(project.join("Cargo.toml"), &manifest).unwrap();
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    // 零 registry 依赖的真实 file:// checkout；没有编译、target 切换或网络 registry 获取。
    let output = checked_output(Command::new(cargo).args(["metadata", "--format-version=1", "--manifest-path"]).arg(project.join("Cargo.toml")).current_dir(&project));
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let packages = metadata["packages"].as_array().unwrap();
    assert_eq!(packages.len(), 10); // 游戏 + SDK + 八项补丁；没有第二份同名包。
    let sdk = packages.iter().find(|package| package["name"] == "feathrise_runtime").unwrap();
    let source = sdk["source"].as_str().unwrap();
    assert!(source.ends_with(&format!("#{revision}")));
    for (name, _) in PATCHES {
        let package = packages.iter().find(|package| package["name"] == name).unwrap();
        assert_eq!(package["source"].as_str(), Some(source));
    }
    assert_eq!(fs::read_to_string(project.join("Cargo.toml")).unwrap(), manifest);
}
