//! 新建工程的 SDK 来源；引擎版本和八项补丁一同绑定到所选 SDK。

use serde::Deserialize;
use std::{
    fs::File,
    io::{ErrorKind, Read},
    path::{Path, PathBuf},
    process::Command,
};
use toml::{Value, map::Map};

const CONFIG_LIMIT: usize = 16 * 1024;
const MANIFEST_LIMIT: usize = 64 * 1024;
const CONFIG_NAME: &str = "sdk-source.toml";
// 随源码的 sdk-source.toml 使用配套检出；单文件安装缺少外置配置时采用已发布配方。
const DEFAULT_CONFIGURATION: &str = include_str!("../sdk-source-public.toml");
const SCENE_SDK_SERIES: &str = "0.3";
const BEVY_PIN: &str = "=0.20.1";
const PATCHES: [(&str, &str); 8] = [
    ("avian3d", "avian3d"),
    ("bevy_anti_alias", "bevy_anti_alias"),
    ("bevy_render", "bevy_render"),
    ("bevy_core_pipeline", "bevy_core_pipeline"),
    ("bevy_extract", "bevy_extract"),
    ("wgpu-hal", "wgpu-hal"),
    ("dlss_wgpu", "dlss_wgpu"),
    ("wgpu-ffx", "wgpu-ffx"),
];

fn validate_bevy(requirement: Option<&str>) -> Result<(), String> {
    match requirement.map(|version| version.trim_start_matches('=')) {
        None | Some("0.20.1") => Ok(()),
        Some(version) => Err(format!("场景工程模板需要 Bevy 0.20.1，SDK 指定了 {version}")),
    }
}

fn scene_sdk_version(value: &str) -> Result<semver::Version, String> {
    let version = semver::Version::parse(value)
        .map_err(|error| format!("场景 SDK 版本必须为完整版本，例如 0.3.0：{error}"))?;
    if (version.major, version.minor) != (0, 3) {
        return Err(format!("场景工程需要 SDK {SCENE_SDK_SERIES}.x，实际配置为 {version}"));
    }
    Ok(version)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Configuration {
    format: u32,
    source: SourceConfiguration,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
enum SourceConfiguration {
    Path { path: String, patch_path: Option<String> },
    Git { git: String, rev: String, patch_git: Option<String>, patch_rev: Option<String>, bevy_version: Option<String> },
    Registry { version: String, git: String, rev: String, bevy_version: Option<String> },
}

pub(super) enum SdkSource {
    Path { root: PathBuf, patches: PathBuf },
    Git { git: String, rev: String, patch_git: String, patch_rev: String },
    Registry { version: String, git: String, rev: String },
}

impl SdkSource {
    pub(super) fn load(configuration_root: &Path) -> Result<Self, String> {
        Self::load_checked(configuration_root).map_err(|error| {
            crate::environment_diagnostics::sdk_configuration_failure(&configuration_root.join(CONFIG_NAME), error)
        })
    }

    fn load_checked(configuration_root: &Path) -> Result<Self, String> {
        // 这是 sdk-source.toml 的实际父目录，不一定是解出的模板/字体等静态资源根。
        let file = configuration_root.join(CONFIG_NAME);
        let text = match File::open(&file) {
            Ok(stream) => read_bounded(stream, CONFIG_LIMIT, &file)?,
            // 只有文件确实不存在才采用随程序的默认配方；无权限、格式错误都不能换来源。
            Err(error) if error.kind() == ErrorKind::NotFound => DEFAULT_CONFIGURATION.to_owned(),
            Err(error) => return Err(format!("SDK 来源配置不可读（{}）：{error}", file.display())),
        };
        let value: Configuration = toml::from_str(&text)
            .map_err(|error| format!("SDK 来源配置无效（{}）：{error}", file.display()))?;
        if value.format != 1 {
            return Err(format!("SDK 来源配置版本不支持：{}", value.format));
        }
        let configuration = value.source;
        match configuration {
            SourceConfiguration::Path { path, patch_path } => {
                validate_text(&path, "SDK 本地路径", 4096)?;
                let root = configuration_root.join(path).canonicalize().map_err(|error| {
                    format!("SDK 本地目录不存在；请配置 {}：{error}", file.display())
                })?;
                let manifest = validate_package(&root, "feathrise_runtime")?;
                let bevy_requirement = manifest.get("dependencies").and_then(|deps| deps.get("bevy"))
                    .and_then(|dependency| dependency.as_str().or_else(|| dependency.get("version").and_then(Value::as_str)))
                    .or_else(|| manifest.get("workspace").and_then(|workspace| workspace.get("dependencies"))
                        .and_then(|deps| deps.get("bevy")).and_then(|dependency| dependency.as_str()
                            .or_else(|| dependency.get("version").and_then(Value::as_str))));
                // 场景模板在写入前检查 SDK 版本，避免生成引用旧地图格式的工程。
                let sdk_version = manifest.get("package").and_then(|package| package.get("version"))
                    .and_then(Value::as_str).ok_or("SDK 缺少版本")?;
                scene_sdk_version(sdk_version)?;
                validate_bevy(bevy_requirement)?;
                let patches = match patch_path {
                    Some(path) => {
                        validate_text(&path, "补丁仓本地路径", 4096)?;
                        configuration_root.join(path)
                    }
                    None => root.with_file_name("feathrise_patches"),
                }.canonicalize().map_err(|error| format!("补丁仓本地目录不可读：{error}"))?;
                for (package, directory) in PATCHES {
                    let patch = patches.join(directory);
                    let actual = patch.canonicalize().map_err(|error| {
                        format!("SDK 补丁 {package} 目录不可读：{error}")
                    })?;
                    if !actual.starts_with(&patches) {
                        return Err(format!("SDK 补丁 {package} 必须位于配置的补丁仓目录内"));
                    }
                    validate_package(&actual, package)?;
                }
                Ok(Self::Path { root, patches })
            }
            SourceConfiguration::Git { git, rev, patch_git, patch_rev, bevy_version } => {
                validate_bevy(bevy_version.as_deref())?;
                let (git, rev) = checked_git_source(git, rev)?;
                let (patch_git, patch_rev) = match (patch_git, patch_rev) {
                    (Some(git), Some(rev)) => checked_git_source(git, rev)?,
                    (None, None) => (git.clone(), rev.clone()), // 同一 Git 修订可同时提供 SDK 和补丁包。
                    _ => return Err("独立补丁仓必须同时提供 patch_git 和 patch_rev".into()),
                };
                Ok(Self::Git { git, rev, patch_git, patch_rev })
            }
            SourceConfiguration::Registry { version, git, rev, bevy_version } => {
                validate_bevy(bevy_version.as_deref())?;
                validate_text(&version, "SDK crates.io 版本", 128)?;
                let version = scene_sdk_version(&version)?;
                let (git, rev) = checked_git_source(git, rev)?;
                // registry 提供实际 SDK；补丁不随 crate 传递，仍显式绑定与此版本配套的公开 Git 修订。
                Ok(Self::Registry { version: version.to_string(), git, rev })
            }
        }
    }

    pub(super) fn apply_template(&self, source: &str) -> Result<String, String> {
        let mut manifest: Value = toml::from_str(source)
            .map_err(|error| format!("空工程 Cargo 模板无效：{error}"))?;
        let dependencies = manifest.get_mut("dependencies").and_then(Value::as_table_mut)
            .ok_or("空工程模板缺少依赖表")?;
        let dependency = dependencies.get_mut("feathrise_runtime")
            .and_then(Value::as_table_mut)
            .ok_or("空工程模板缺少显式 feathrise_runtime 依赖表")?;
        // 保留模板的 features/default-features 等能力声明；仅替换依赖来源。
        for key in ["path", "git", "rev", "branch", "tag", "registry", "version", "workspace"] {
            dependency.remove(key);
        }
        dependency.extend(self.dependency(None)?);
        if let Some(dependency) = dependencies.get_mut("bevy") {
            // 自由相机组件须与所选 SDK 的 App 来自同一套 Bevy。
            let table = dependency.as_table_mut().ok_or("空工程模板的 Bevy 依赖必须声明能力表")?;
            table.insert("version".into(), Value::String(BEVY_PIN.into()));
        }
        let root = manifest.as_table_mut().ok_or("空工程 Cargo 模板必须是表")?;
        if root.contains_key("workspace") || root.contains_key("patch") {
            return Err("空工程模板不能另行声明 workspace 或 patch；SDK 配置统一生成".into());
        }
        root.insert("workspace".into(), Value::Table(Map::from_iter([
            ("resolver".into(), Value::String("3".into())),
        ])));
        let mut patches = Map::new();
        for (package, directory) in PATCHES {
            patches.insert(package.into(), Value::Table(self.dependency(Some(directory))?));
        }
        root.insert("patch".into(), Value::Table(Map::from_iter([
            ("crates-io".into(), Value::Table(patches)),
        ])));
        toml::to_string_pretty(&manifest).map_err(|error| error.to_string())
    }

    fn dependency(&self, patch: Option<&str>) -> Result<Map<String, Value>, String> {
        Ok(match self {
            Self::Path { root, patches, .. } => {
                let path = patch.map_or_else(|| root.clone(), |name| patches.join(name));
                Map::from_iter([("path".into(), Value::String(super::toml_path(&path)?))])
            }
            Self::Git { git, rev, patch_git, patch_rev, .. } => {
                let (url, revision) = if patch.is_some() { (patch_git, patch_rev) } else { (git, rev) };
                let mut dependency = Map::from_iter([
                    ("git".into(), Value::String(url.clone())),
                    ("rev".into(), Value::String(revision.clone())),
                ]);
                // 锁定 Git 提交仍须声明场景 SDK 系列；Cargo 会拒绝同名旧 SDK。
                if patch.is_none() {
                    dependency.insert("version".into(), Value::String(SCENE_SDK_SERIES.into()));
                }
                dependency
            }
            Self::Registry { version, git, rev, .. } => match patch {
                Some(_) => Map::from_iter([
                    ("git".into(), Value::String(git.clone())),
                    ("rev".into(), Value::String(rev.clone())),
                ]),
                // 与原 Git 固定提交策略一致，新工程不能悄悄升级至另一份 SDK／补丁组合。
                None => Map::from_iter([("version".into(), Value::String(format!("={version}")))]),
            },
        })
    }

    pub(super) fn apply_network_policy(&self, command: &mut Command) {
        if matches!(self, Self::Path { .. }) {
            command.arg("--offline");
        }
    }

}

fn checked_git_source(git: String, rev: String) -> Result<(String, String), String> {
    validate_text(&git, "SDK Git 地址", 4096)?;
    if !["https://", "http://", "ssh://", "git://", "file://"]
        .iter().any(|prefix| git.starts_with(prefix))
    {
        return Err("SDK Git 地址需使用 https://、http://、ssh://、git:// 或显式 file:// URL".into());
    }
    if git.chars().any(char::is_whitespace) || git.contains(['#', '?']) {
        return Err("SDK Git 地址不能包含空白、查询或片段；提交只由 rev 指定".into());
    }
    if rev.len() != 40 || !rev.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("SDK rev 必须为完整 40 位十六进制提交 ID，不接受分支、标签或短 rev".into());
    }
    Ok((git, rev.to_ascii_lowercase()))
}

fn validate_text(value: &str, label: &str, limit: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.trim() != value || value.len() > limit || value.chars().any(char::is_control) {
        return Err(format!("{label} 必须非空、不含控制字符或首尾空白且不超过 {limit} 字节"));
    }
    Ok(())
}

fn validate_package(root: &Path, package: &str) -> Result<Value, String> {
    let manifest = read_manifest(&root.join("Cargo.toml"))?;
    if manifest.get("package").and_then(|value| value.get("name")).and_then(Value::as_str) != Some(package) {
        return Err(format!("SDK 包身份错误：{} 应声明 package.name={package}", root.display()));
    }
    Ok(manifest)
}

fn read_bounded(file: File, limit: usize, path: &Path) -> Result<String, String> {
    let mut text = String::new();
    file.take((limit + 1) as u64).read_to_string(&mut text)
        .map_err(|error| format!("文件不可读（{}）：{error}", path.display()))?;
    if text.len() > limit {
        return Err(format!("文件超过 {} KiB 读取预算：{}", limit / 1024, path.display()));
    }
    Ok(text)
}

fn read_manifest(path: &Path) -> Result<Value, String> {
    let file = File::open(path).map_err(|error| format!("Cargo 清单不可读（{}）：{error}", path.display()))?;
    toml::from_str(&read_bounded(file, MANIFEST_LIMIT, path)?)
        .map_err(|error| format!("Cargo 清单无效（{}）：{error}", path.display()))
}

/// 工程预览按当前工程清单中的依赖决定取源策略，避免套用新建工程的配置。
/// 未知/继承的工作区或自定义依赖交给 Cargo；已知纯本地 SDK 保持原离线行为。
/// 返回同一次有界解析的清单，供构建入口核对工程默认特性。
pub(crate) fn apply_project_network_policy(manifest: &Path, command: &mut Command) -> Result<Value, String> {
    let value = read_manifest(manifest)?;
    let known_local = value.get("dependencies").and_then(Value::as_table).is_some_and(|table| {
        table.iter().any(|(alias, dependency)| {
            let package = dependency.get("package").and_then(Value::as_str).unwrap_or(alias);
            matches!(package, "feathrise_runtime" | "bevy_feathrise" | "feathrise_project_runtime" | "bevy_feathrise_project" | "new_editor_game_runtime")
                && dependency.get("path").and_then(Value::as_str).is_some()
        })
    });
    if known_local && !needs_cargo_source_resolution(&value) {
        command.arg("--offline");
    }
    Ok(value)
}

fn needs_cargo_source_resolution(value: &Value) -> bool {
    match value {
        Value::Table(table) => table.get("git").and_then(Value::as_str).is_some()
            || table.get("workspace").and_then(Value::as_bool) == Some(true)
            || table.values().any(needs_cargo_source_resolution),
        Value::Array(values) => values.iter().any(needs_cargo_source_resolution),
        _ => false,
    }
}

#[cfg(test)]
#[path = "project_template_sdk_tests.rs"]
mod tests;
