//! 宿主可写位置：构建缓存和个人偏好归用户；工程事务仍由工程本身拥有。
//!
//! 这里只在启动阶段解析路径，不创建目录、不搬迁缓存，也不把安装资源目录当作写入探测点。
//! 正常日志使用已有有界管道，临时作者文件继续在工程内原子发布，无需预建无人使用的目录。

#[cfg(test)]
mod tests;

use crate::resource_paths::ResourceMode;
use std::path::{Component, Path, PathBuf};

const MAX_PATH_BYTES: usize = 8192;

/// 覆盖顺序固定为命令行、Cargo 环境、运行模式默认值；子作者随后收到同一绝对路径。
pub(crate) fn build_target(mode: ResourceMode, explicit: Option<PathBuf>) -> Result<PathBuf, String> {
    let environment = std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from);
    if let Some((path, label)) = selected_override(explicit.as_deref(), environment.as_deref()) {
        // 只在确有相对覆盖时读取 cwd；固定后 F12 或作者实例切换 cwd 不会改变缓存身份。
        let cwd = if path.is_absolute() {
            PathBuf::new()
        } else {
            std::env::current_dir().map_err(|error| format!("无法解析 {label} 的启动目录：{error}"))?
        };
        return absolute_target(path, &cwd, label);
    }
    let resources = crate::project_template_root();
    if mode == ResourceMode::Development {
        return default_target(mode, &resources, None, None);
    }
    let variable = if cfg!(windows) { "LOCALAPPDATA" } else { "XDG_CACHE_HOME" };
    let local_data = std::env::var_os(variable).map(PathBuf::from);
    let home = if local_data.is_none() { Some(user_home()?) } else { None };
    default_target(mode, &resources, local_data.as_deref(), home.as_deref())
}

fn selected_override<'a>(
    explicit: Option<&'a Path>,
    environment: Option<&'a Path>,
) -> Option<(&'a Path, &'static str)> {
    explicit.map(|path| (path, "--target-dir"))
        .or_else(|| environment.map(|path| (path, "CARGO_TARGET_DIR")))
}

fn absolute_target(path: &Path, cwd: &Path, label: &str) -> Result<PathBuf, String> {
    if path.as_os_str().is_empty() {
        return Err(format!("{label} 不能为空；请填写构建缓存目录或移除此覆盖"));
    }
    if path.is_absolute() {
        return Ok(path.to_owned());
    }
    if path.components().any(|part| matches!(part, Component::Prefix(_) | Component::RootDir)) {
        return Err(format!("{label} 不能只指定盘符或根目录：{}；请使用完整绝对路径", path.display()));
    }
    if !cwd.is_absolute() {
        return Err(format!("无法取得 {label} 的绝对启动目录"));
    }
    // 不 canonicalize 尚不存在的缓存，也不折叠可能经过链接的父级路径；保留显式覆盖语义。
    Ok(cwd.join(path))
}

fn default_target(
    mode: ResourceMode,
    resources: &Path,
    local_data: Option<&Path>,
    home: Option<&Path>,
) -> Result<PathBuf, String> {
    if mode == ResourceMode::Development {
        validate_absolute(resources, "开发资源目录")?;
        return Ok(resources.join("target"));
    }
    let root = if let Some(path) = local_data {
        let variable = if cfg!(windows) { "LOCALAPPDATA" } else { "XDG_CACHE_HOME" };
        validate_absolute(path, variable)?;
        path.to_owned()
    } else {
        let home = home.ok_or("无法定位用户缓存目录；请设置 LOCALAPPDATA 或显式指定 --target-dir")?;
        validate_absolute(home, "用户主目录")?;
        // 环境变量缺席才采用平台标准位置；错误的显式变量不能悄悄改变缓存来源。
        if cfg!(windows) { home.join("AppData/Local") } else { home.join(".cache") }
    };
    Ok(root.join("Feathrise/target"))
}

pub(crate) fn user_home() -> Result<PathBuf, String> {
    let variable = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    let home = std::env::var_os(variable)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| format!("无法确定用户主目录：环境变量{variable}未设置"))?;
    validate_absolute(&home, "用户主目录")?;
    Ok(home)
}

/// 个人配置与全局插件统一归用户主目录的 .feathrise；不随编辑器或工程位置改变。
pub(crate) fn preferences_file(home: &Path) -> Result<PathBuf, String> {
    validate_absolute(home, "用户主目录")?;
    Ok(home.join(".feathrise/editor.json"))
}

/// 旧地址只用于首次迁移；新配置存在时不读取旧地址，也不因旧环境变量异常阻断新配置。
pub(crate) fn legacy_preferences_file(home: &Path) -> PathBuf {
    #[cfg(windows)]
    let roaming = std::env::var_os("APPDATA").filter(|value| !value.is_empty()).map(PathBuf::from);
    #[cfg(not(windows))]
    let roaming: Option<PathBuf> = None;
    legacy_preferences_file_at(home, roaming.as_deref())
}

fn legacy_preferences_file_at(home: &Path, roaming: Option<&Path>) -> PathBuf {
    if let Some(root) = roaming {
        return root.join("Feathrise/editor.json");
    }
    home.join(".config/feathrise/editor.json")
}

/// 沿用个人偏好的既有词法约束；不访问目录或改变作者选择的路径。
pub(crate) fn validate_absolute(path: &Path, label: &str) -> Result<(), String> {
    let text = path.to_str().ok_or_else(|| "目录名称不是有效UTF-8".to_owned())?;
    if !path.is_absolute()
        || text.len() > MAX_PATH_BYTES
        || text.chars().any(char::is_control)
        // Windows verbatim 路径在 join 时可能折叠 ..，组合前后的词法检查都保留。
        || text.split(['/', '\\']).any(|segment| matches!(segment, "." | ".."))
        || path.components().any(|component| matches!(component, Component::CurDir | Component::ParentDir))
    {
        return Err(format!("{label}必须是无父级跳转的绝对目录，长度不超过{MAX_PATH_BYTES}字节"));
    }
    Ok(())
}
