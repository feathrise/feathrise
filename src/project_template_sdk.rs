//! 新建工程的 SDK 来源；引擎版本和八项补丁一同绑定到所选 SDK。

use super::manifest::{self, PATCHES, scene_registry_sdk_version};
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
// 两份内嵌文件由构建配置成对选择。锁属于新建工程模板，来源校验与外置配置共用。
const DISTRIBUTION_CONFIGURATION: &str = include_str!(env!("FEATHRISE_TEMPLATE_RECIPE_PATH"));
const DISTRIBUTION_LOCK: &[u8] = include_bytes!(env!("FEATHRISE_TEMPLATE_LOCK_PATH"));

fn validate_bevy(requirement: Option<&str>) -> Result<(), String> {
    match requirement.map(|version| version.trim_start_matches('=')) {
        None | Some("0.20.1") => Ok(()),
        Some(version) => Err(format!("场景工程模板需要 Bevy 0.20.1，SDK 指定了 {version}")),
    }
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

#[derive(PartialEq, Eq)]
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
            // 只有配置缺席时采用可独立分发的默认值；坏配置仍明确失败，不能悄悄改换 SDK。
            Err(error) if error.kind() == ErrorKind::NotFound => return Self::distribution(),
            Err(error) => return Err(format!("SDK 来源配置不可读（{}）：{error}", file.display())),
        };
        Self::parse_configuration(&text, &file)
    }

    fn distribution() -> Result<Self, String> {
        Self::parse_configuration(DISTRIBUTION_CONFIGURATION, Path::new("sdk-source-distribution.toml"))
    }

    pub(super) fn distribution_lock(&self) -> Option<&'static [u8]> {
        // 只有所选来源与内嵌配方一致才用配套锁。显式新来源仍自行解析锁文件。
        let bundled = Self::distribution().ok()?;
        (self == &bundled).then_some(DISTRIBUTION_LOCK)
    }

    fn parse_configuration(text: &str, file: &Path) -> Result<Self, String> {
        let configuration: Configuration = toml::from_str(text)
            .map_err(|error| format!("SDK 来源配置无效（{}）：{error}", file.display()))?;
        if configuration.format != 1 {
            return Err(format!("SDK 来源配置版本不支持：{}", configuration.format));
        }
        match configuration.source {
            SourceConfiguration::Path { path, patch_path } => {
                let configuration_root = file.parent().ok_or("SDK 配置缺少所在目录")?;
                validate_text(&path, "SDK 本地路径", 4096)?;
                let root = configuration_root.join(path).canonicalize().map_err(|error| {
                    format!("SDK 本地目录不存在；请配置 {}：{error}", file.display())
                })?;
                let manifest = validate_package(&root, "feathrise_runtime")?;
                let dependencies = manifest.get("dependencies");
                let workspace_dependencies = manifest.get("workspace").and_then(|workspace| workspace.get("dependencies"));
                let bevy_requirement = dependencies.and_then(|deps| deps.get("bevy"))
                    .and_then(|dependency| dependency.as_str().or_else(|| dependency.get("version").and_then(Value::as_str)))
                    .or_else(|| workspace_dependencies.and_then(|deps| deps.get("bevy"))
                        .and_then(|dependency| dependency.as_str()
                            .or_else(|| dependency.get("version").and_then(Value::as_str))));
                // 开发源码与旧地图包可使用相同版本号，按实际依赖和包身份确认场景合同。
                let content = dependencies.and_then(|deps| deps.get("feathrise_scene_content"));
                let content = match content {
                    Some(dependency) if dependency.get("workspace").and_then(Value::as_bool) == Some(true) =>
                        workspace_dependencies.and_then(|deps| deps.get("feathrise_scene_content")),
                    _ => content,
                };
                let terrain = workspace_dependencies.and_then(|deps| deps.get("feathrise_terrain"));
                for (package, dependency) in [("feathrise_scene_content", content), ("feathrise_terrain", terrain)] {
                    let relative = dependency.and_then(|dep| dep.get("path")).and_then(Value::as_str)
                        .ok_or_else(|| format!("场景 SDK 缺少 {package} 本地依赖"))?;
                    let package_root = root.join(relative).canonicalize()
                        .map_err(|error| format!("场景 SDK 包 {package} 的目录不可读（{error}）"))?;
                    if !package_root.starts_with(&root) {
                        return Err(format!("场景 SDK 包 {package} 必须位于配置的 SDK 目录内"));
                    }
                    validate_package(&package_root, package)?;
                }
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
                let version = scene_registry_sdk_version(&version)?;
                let (git, rev) = checked_git_source(git, rev)?;
                // registry 提供实际 SDK；补丁不随 crate 传递，仍显式绑定与此版本配套的公开 Git 修订。
                Ok(Self::Registry { version: version.to_string(), git, rev })
            }
        }
    }

    pub(super) fn apply_template(&self, source: &str) -> Result<String, String> {
        let mut patches = Map::new();
        for (package, directory) in PATCHES {
            patches.insert(package.into(), Value::Table(self.dependency(Some(directory))?));
        }
        manifest::apply_template(source, self.dependency(None)?, patches)
    }

    fn dependency(&self, patch: Option<&str>) -> Result<Map<String, Value>, String> {
        Ok(match self {
            Self::Path { root, patches, .. } => {
                let path = patch.map_or_else(|| root.clone(), |name| patches.join(name));
                Map::from_iter([("path".into(), Value::String(super::toml_path(&path)?))])
            }
            Self::Git { git, rev, patch_git, patch_rev, .. } => {
                let (url, revision) = if patch.is_some() { (patch_git, patch_rev) } else { (git, rev) };
                // 源码以完整提交锁定，允许选择仍沿用已有包版本号的场景改造。
                Map::from_iter([
                    ("git".into(), Value::String(url.clone())),
                    ("rev".into(), Value::String(revision.clone())),
                ])
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
