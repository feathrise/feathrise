//! 插件目录快捷打开：纯路径供界面显示，创建与文件管理器启动只在后台执行。
use super::types::{PluginRoots, PluginScope};
use std::{fs, path::{Component, Path, PathBuf}, process::Command};

/// 呈现路径时不读磁盘；范围切换使用同一份冻结来源，避免打开另一个工程的目录。
pub(crate) fn path(roots: &PluginRoots, scope: PluginScope) -> Result<PathBuf, String> {
    let path = match scope {
        PluginScope::Global => roots.global.clone(),
        PluginScope::Project => roots.project.as_ref().ok_or("尚未打开工程")?.join("plugins"),
    };
    if !path.is_absolute() || path.components().any(|part| matches!(part, Component::ParentDir)) {
        return Err("插件目录必须是明确的绝对路径".into());
    }
    Ok(path)
}

pub(crate) fn open(roots: &PluginRoots, scope: PluginScope) -> Result<(), String> {
    let directory = prepare(&path(roots, scope)?)?;
    #[cfg(target_os = "windows")]
    let mut command = Command::new("explorer.exe");
    #[cfg(target_os = "macos")]
    let mut command = Command::new("open");
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let mut command = Command::new("xdg-open");
    // 路径作为单独参数交给系统，不拼接shell命令，目录名包含空格或中文也保持完整。
    // Explorer使用已识别的盘符/UNC表示；文件校验与创建仍使用上面的原始身份路径。
    #[cfg(target_os = "windows")]
    command.arg(crate::project::display_path(&directory));
    #[cfg(not(target_os = "windows"))]
    command.arg(directory);
    command.spawn().map(|_| ())
        .map_err(|error| format!("无法打开插件目录：{error}"))
}

fn prepare(path: &Path) -> Result<PathBuf, String> {
    let ancestors = path.ancestors().collect::<Vec<_>>();
    if ancestors.len() > 128 {
        return Err("插件目录层级超出预算".into());
    }
    for ancestor in ancestors.into_iter().rev() {
        let metadata = match fs::symlink_metadata(ancestor) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(ancestor).map_err(|error| format!("无法创建插件目录：{error}"))?;
                fs::symlink_metadata(ancestor).map_err(|error| error.to_string())?
            }
            Err(error) => return Err(format!("无法检查插件目录：{error}")),
        };
        #[cfg(windows)]
        let reparse = {
            use std::os::windows::fs::MetadataExt;
            metadata.file_attributes() & 0x400 != 0
        };
        #[cfg(not(windows))]
        let reparse = false;
        if !metadata.is_dir() || metadata.file_type().is_symlink() || reparse {
            return Err("插件目录及父目录不能是文件、符号链接或目录联接".into());
        }
    }
    path.canonicalize().map_err(|error| format!("无法定位插件目录：{error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_selects_the_actual_global_or_project_directory_without_reading_disk() {
        let root = std::env::temp_dir().join("feathrise-directory-path-fixture");
        let roots = PluginRoots { project: Some(root.join("project")), global: root.join("global") };
        assert_eq!(path(&roots, PluginScope::Global).unwrap(), roots.global);
        assert_eq!(path(&roots, PluginScope::Project).unwrap(), root.join("project/plugins"));
        let none = PluginRoots { project: None, global: roots.global };
        assert!(path(&none, PluginScope::Project).is_err());
        assert!(path(&PluginRoots { project: None, global: "relative".into() }, PluginScope::Global).is_err());
    }

    #[test]
    fn prepare_creates_owned_directories_and_preserves_a_conflicting_file() {
        let root = std::env::temp_dir().join(format!("feathrise-directory-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        fs::create_dir(&root).unwrap();
        let owned = root.canonicalize().unwrap();
        fs::write(owned.join("owner"), b"plugin-directory-fixture").unwrap();
        let directory = owned.join("new/plugins");
        assert_eq!(prepare(&directory).unwrap(), directory.canonicalize().unwrap());
        let file = owned.join("file");
        fs::write(&file, b"must-survive").unwrap();
        assert!(prepare(&file.join("plugins")).is_err());
        assert_eq!(fs::read(&file).unwrap(), b"must-survive");
        // 仅回收本轮成功创建并仍具有精确归属标记的临时根，不碰用户插件目录。
        assert_eq!(owned.parent().unwrap(), std::env::temp_dir().canonicalize().unwrap());
        assert_eq!(fs::read(owned.join("owner")).unwrap(), b"plugin-directory-fixture");
        fs::remove_dir_all(owned).unwrap();
    }
}
