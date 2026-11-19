//! 发现本机 Blender，供网格往返、绑骨等显式工具任务复用。

use std::path::{Path, PathBuf};

const BLENDER_PATH_ENVIRONMENT: &str = "BEVY_LEARN5_BLENDER_PATH";

/// 定位优先级为进程覆盖、个人偏好、PATH，再到 Windows 标准安装目录。
pub(crate) fn find_blender_executable(saved: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(configured) = std::env::var_os(BLENDER_PATH_ENVIRONMENT) {
        if configured.is_empty() {
            return Err(format!(
                "环境变量{BLENDER_PATH_ENVIRONMENT}为空；请删除它或填写blender.exe绝对路径"
            )); // 显式配置错误不得静默切换成另一版本。
        }
        let path = PathBuf::from(configured);
        ensure_nonempty_regular_file(&path, "显式配置的Blender可执行文件")?;
        return Ok(path);
    }
    if let Some(path) = saved {
        ensure_nonempty_regular_file(path, "已保存的 Blender 程序")
            .map_err(|error| format!("{error}。请在「设置 → 常规 → Blender 程序」重新选择或使用自动查找。"))?;
        return Ok(path.to_owned());
    }
    let executable_name = if cfg!(windows) { "blender.exe" } else { "blender" };
    if let Some(path_value) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&path_value) {
            let candidate = directory.join(executable_name);
            if is_nonempty_regular_file(&candidate) {
                return Ok(candidate); // PATH 顺序保留作者选择的版本优先级。
            }
        }
    }
    #[cfg(windows)]
    if let Some(candidate) = newest_windows_blender_installation() {
        return Ok(candidate);
    }
    Err(format!("未找到 Blender。请在「设置 → 常规 → Blender 程序」选择，或设置 {BLENDER_PATH_ENVIRONMENT}。"))
}

/// 只扫描两个标准根的直接子目录，按数值版本选择最新安装。
#[cfg(windows)]
fn newest_windows_blender_installation() -> Option<PathBuf> {
    let mut roots = Vec::new();
    for variable in ["ProgramFiles", "ProgramW6432"] {
        if let Some(value) = std::env::var_os(variable) {
            let root = PathBuf::from(value).join("Blender Foundation");
            if !roots.contains(&root) {
                roots.push(root);
            }
        }
    }
    let mut candidates = Vec::new();
    for root in roots {
        let Ok(entries) = std::fs::read_dir(root) else { continue; };
        for entry in entries.flatten() {
            let candidate = entry.path().join("blender.exe");
            if is_nonempty_regular_file(&candidate) {
                candidates.push(candidate);
            }
        }
    }
    candidates.sort_by_key(|path| blender_version_rank(path));
    candidates.pop()
}

/// 官方父目录名携带主、次、补丁版本；路径用于相同版本的确定性排序。
#[cfg(any(windows, test))]
fn blender_version_rank(path: &Path) -> (u32, u32, u32, String) {
    let name = path.parent().and_then(Path::file_name).and_then(|value| value.to_str())
        .unwrap_or_default();
    let version = name.strip_prefix("Blender ").unwrap_or_default();
    let mut parts = version.split('.').map(|part| part.parse::<u32>().unwrap_or(0));
    (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        path.to_string_lossy().into_owned(),
    )
}

fn ensure_nonempty_regular_file(path: &Path, label: &str) -> Result<(), String> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| format!("读取{label}{}失败：{error}", path.display()))?;
    if !metadata.file_type().is_file() || metadata.len() == 0 {
        return Err(format!("{label}必须是非空普通文件：{}", path.display()));
    }
    Ok(())
}

fn is_nonempty_regular_file(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .is_ok_and(|metadata| metadata.file_type().is_file() && metadata.len() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_blender_versions_sort_newest_first_candidate_last() {
        let older = Path::new("C:/Program Files/Blender Foundation/Blender 4.3/blender.exe");
        let newer = Path::new("C:/Program Files/Blender Foundation/Blender 5.1/blender.exe");
        assert!(blender_version_rank(newer) > blender_version_rank(older));
    }
}
