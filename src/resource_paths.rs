//! 启动时选择并校验编辑器资源根；内嵌图标、模板和热补丁代理不受 cwd 影响。

#[cfg(test)]
mod tests;
mod embedded;

use feathrise_editor_core::{EditorResourcePaths, bind_editor_resource_paths};
use serde::Deserialize;
use std::{collections::BTreeSet, fs, path::{Path, PathBuf}, sync::OnceLock};

// SDK 来源属于配置位置，不能随未来的静态资源缓存搬迁而改变相对路径含义。
static SDK_CONFIGURATION_ROOT: OnceLock<PathBuf> = OnceLock::new();

pub(crate) fn sdk_configuration_root(resource_root: &Path) -> PathBuf {
    SDK_CONFIGURATION_ROOT.get().cloned().unwrap_or_else(|| resource_root.to_owned())
}

/// 启动和分发工具共享唯一清单；编译嵌入使损坏安装不能靠修改磁盘清单绕过检查。
const RESOURCE_MANIFEST: &str = include_str!("../build/runtime_resources.json");

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResourceManifest {
    format: u32,
    required_files: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ResourceMode {
    Development,
    Installed,
}

// 固定带界标记供分发工具只读 PE 校验真实编译来源，不执行用户提供的 exe。
// 包模式不能通过把 debug exe 移到名为 release 的目录来伪装。
macro_rules! build_identity {
    ($upscaling:literal, $frame_generation:literal) => {
        #[cfg(debug_assertions)]
        static BUILD_IDENTITY: &str = concat!("FEATHRISE_BUILD_IDENTITY_V2\0", env!("OUT_DIR"), "\0", "1\0", env!("CARGO_PKG_VERSION"), "\0", $upscaling, "\0", $frame_generation, "\0FEATHRISE_BUILD_IDENTITY_END");
        #[cfg(not(debug_assertions))]
        static BUILD_IDENTITY: &str = concat!("FEATHRISE_BUILD_IDENTITY_V2\0", env!("OUT_DIR"), "\0", "0\0", env!("CARGO_PKG_VERSION"), "\0", $upscaling, "\0", $frame_generation, "\0FEATHRISE_BUILD_IDENTITY_END");
    };
}
// 构建事实和用户当前画质设置分别处理；有组件才组包，不替用户启用候选。
#[cfg(all(feature = "fsr3_upscaling", feature = "fsr3_frame_generation"))]
build_identity!("1", "1");
#[cfg(all(feature = "fsr3_upscaling", not(feature = "fsr3_frame_generation")))]
build_identity!("1", "0");
#[cfg(all(not(feature = "fsr3_upscaling"), feature = "fsr3_frame_generation"))]
build_identity!("0", "1");
#[cfg(all(not(feature = "fsr3_upscaling"), not(feature = "fsr3_frame_generation")))]
build_identity!("0", "0");

fn required_files(source: &str) -> Result<Vec<String>, String> {
    if source.len() > 64 * 1024 {
        return Err("编辑器资源清单超过 64 KiB".into());
    }
    let manifest: ResourceManifest = serde_json::from_str(source)
        .map_err(|error| format!("编辑器资源清单无效：{error}"))?;
    if manifest.format != 1 || manifest.required_files.is_empty() || manifest.required_files.len() > 128 {
        return Err("编辑器资源清单版本或条目数量无效".into());
    }
    let mut names = BTreeSet::new();
    for path in &manifest.required_files {
        if path.len() > 512 || path.contains(['\\', ':']) || path.chars().any(char::is_control)
            || path.split('/').any(|part| part.is_empty() || part == "." || part == "..")
            || !names.insert(path.to_lowercase())
        {
            return Err(format!("编辑器资源清单包含不安全或重复路径：{path}"));
        }
    }
    Ok(manifest.required_files)
}

pub(crate) fn required_resource_files() -> Result<Vec<String>, String> {
    required_files(RESOURCE_MANIFEST)
}

pub(crate) fn initialize() -> Result<ResourceMode, String> {
    // 保留完整的只读构建标记，即使优化构建也不由链接器移除未使用字符串。
    std::hint::black_box(BUILD_IDENTITY);
    embedded::retain_identity();
    #[cfg(feature = "fsr3_frame_generation")]
    std::hint::black_box(feathrise_fsr3_frame_generation::distribution_build_identity());
    let executable = std::env::current_exe()
        .map_err(|error| format!("无法定位 Feathrise 可执行文件：{error}"))?;
    let source_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    // OUT_DIR 为 <target>/<profile>/build/<package>/out；精确记录实际编译输出位置，
    // 支持既有自定义 target-dir，但复制出去的 exe 不能据此借用开发机源码。
    let build_directory = Path::new(env!("OUT_DIR"))
        .ancestors()
        .nth(3)
        .ok_or("编译时编辑器输出目录结构无效")?;
    let modern = std::env::var_os("FEATHRISE_EDITOR_ROOT").map(PathBuf::from);
    let legacy = std::env::var_os("EDITOR_HOST_REPO_ROOT").map(PathBuf::from);
    let root = resolve(
        &executable,
        modern.as_deref(),
        legacy.as_deref(),
        source_root,
        build_directory,
        embedded::materialize,
    )?;
    let mode = resource_mode(&root, &executable, source_root, build_directory);
    let configuration = if modern.is_some() || legacy.is_some() || mode == ResourceMode::Development {
        root.clone()
    } else {
        executable.parent().ok_or("EXE 缺少配置父目录")?.to_owned()
    };
    if let Some(previous) = SDK_CONFIGURATION_ROOT.get() {
        if previous != &configuration {
            return Err("本进程已绑定其他 SDK 配置目录；请重启后更换来源".into());
        }
    } else {
        SDK_CONFIGURATION_ROOT.set(configuration).map_err(|_| "SDK 配置目录重复初始化")?;
    }
    bind_editor_resource_paths(EditorResourcePaths::new(root)?)?;
    Ok(mode)
}

fn resource_mode(root: &Path, executable: &Path, source_root: &Path, build_directory: &Path) -> ResourceMode {
    if source_root.canonicalize().ok().as_deref() == Some(root)
        && executable.parent().and_then(|path| path.canonicalize().ok()) == build_directory.canonicalize().ok()
        && build_directory.is_dir()
    {
        ResourceMode::Development
    } else {
        ResourceMode::Installed
    }
}

/// 所有候选均为明确来源；选定候选损坏即报错，绝不从另一个目录拼成混合资源包。
fn resolve(
    executable: &Path,
    modern_override: Option<&Path>,
    legacy_override: Option<&Path>,
    source_root: &Path,
    build_directory: &Path,
    embedded: impl FnOnce() -> Result<PathBuf, String>,
) -> Result<PathBuf, String> {
    if let Some(path) = modern_override {
        return validate_root(path, "FEATHRISE_EDITOR_ROOT");
    }
    if let Some(path) = legacy_override {
        if !path.is_absolute() {
            return Err("EDITOR_HOST_REPO_ROOT 必须填写绝对路径；相对目录会随启动位置改变".into());
        }
        // 旧变量既接受原开发仓库，也接受编辑器根；只保留这条已使用的兼容约定。
        let nested = path.join("feathrise");
        let root = if nested.is_dir() && !path.join("assets/templates").exists() {
            nested
        } else {
            path.to_owned()
        };
        return validate_root(&root, "EDITOR_HOST_REPO_ROOT");
    }
    let directory = executable.parent().ok_or("Feathrise 可执行文件缺少父目录")?;
    let directory = directory.canonicalize()
        .map_err(|error| format!("无法访问 Feathrise 安装目录 {}：{error}", directory.display()))?;
    // 新内嵌程序不自动采用 EXE 旁的 assets：那可能是工程内容或旧版资源。
    // 外置覆盖只接受上面的显式选项，避免升级 EXE 却仍执行旧模板/脚本。
    if build_directory.canonicalize().ok().as_deref() == Some(directory.as_path())
        && source_root.try_exists().map_err(|error| format!("无法检查源码资源目录：{error}"))?
    {
        // 仅原编译输出里的源码开发入口使用源码资源，且同样经过完整性校验。
        return validate_root(source_root, "源码开发资源");
    }
    embedded()
}

fn validate_root(root: &Path, source: &str) -> Result<PathBuf, String> {
    if !root.is_absolute() {
        return Err(format!("{source} 必须填写绝对路径；相对目录会随启动位置改变"));
    }
    let root = root.canonicalize()
        .map_err(|error| format!("{source} 无法访问编辑器资源根 {}：{error}", root.display()))?;
    if !root.is_dir() {
        return Err(format!("{source} 的编辑器资源根必须是目录：{}", root.display()));
    }
    // 只检查固定清单的元数据和可读性，既不加载大字体，也不在运行帧里再次执行。
    let mut failures = Vec::new();
    for relative in required_resource_files()? {
        let path = root.join(&relative);
        let checked = fs::metadata(&path)
            .and_then(|metadata| {
                if !metadata.is_file() || metadata.len() == 0 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "必须是非空普通文件",
                    ));
                }
                fs::File::open(&path).map(|_| ())
            });
        if let Err(error) = checked {
            failures.push(format!("{relative}：{error}"));
        }
    }
    if !failures.is_empty() {
        return Err(format!(
            "{source} 的 Feathrise 资源不完整：{}\n{}\n请恢复此资源包；不会回退其他目录。",
            root.display(), failures.join("\n"),
        ));
    }
    Ok(root)
}
