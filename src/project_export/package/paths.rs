//! 导出路径校验；在创建目录前拒绝资产重叠、父路径覆盖与链接跳转。

use crate::project::Project;
use std::{fs, path::{Component, Path, PathBuf}};

pub(super) fn ordinary(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 { return false; }
    }
    !metadata.file_type().is_symlink()
}

pub(super) fn relative(path: &Path) -> Result<(), String> {
    if path.as_os_str().is_empty() || path.components().count() > 64 {
        return Err("导出相对路径不能为空或超过64层".into());
    }
    for component in path.components() {
        let Component::Normal(name) = component else { return Err("导出路径不能是绝对路径、当前目录或父跳转".into()); };
        let name = name.to_str().ok_or("导出路径必须使用UTF-8名称")?;
        let stem = name.split('.').next().unwrap_or_default().to_ascii_uppercase();
        let device = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || stem.strip_prefix("COM").or_else(|| stem.strip_prefix("LPT"))
                .is_some_and(|value| value.len() == 1 && matches!(value.as_bytes()[0], b'1'..=b'9'));
        if name.len() > 255 || name.contains([':', '<', '>', '"', '|', '?', '*'])
            || name.ends_with(['.', ' ']) || name.chars().any(char::is_control) || device {
            return Err("导出路径包含不安全的Windows名称".into());
        }
    }
    Ok(())
}

pub(super) fn checked(path: &Path, directory: bool) -> Result<PathBuf, String> {
    if !path.is_absolute() { return Err("导出文件校验需要绝对路径".into()); }
    let mut current = PathBuf::new();
    for component in path.components() {
        if matches!(component, Component::ParentDir | Component::CurDir) { return Err("导出路径不能含父跳转或当前目录".into()); }
        current.push(component);
        if matches!(component, Component::Prefix(_)) { continue; }
        let metadata = fs::symlink_metadata(&current).map_err(|error| format!("导出路径不可访问：{}：{error}", current.display()))?;
        if !ordinary(&metadata) { return Err(format!("导出不接受符号链接或junction：{}", current.display())); }
    }
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if metadata.is_dir() != directory || (!directory && !metadata.is_file()) { return Err("导出来源类型已变化".into()); }
    path.canonicalize().map_err(|error| error.to_string())
}

pub(super) fn contains(root: &Path, path: &Path) -> bool {
    let mut root = root.components();
    let mut candidate = path.components();
    loop {
        match (root.next(), candidate.next()) {
            (None, _) => return true,
            (Some(left), Some(right)) => {
                #[cfg(windows)]
                let equal = left.as_os_str().to_string_lossy().eq_ignore_ascii_case(&right.as_os_str().to_string_lossy());
                #[cfg(not(windows))]
                let equal = left == right;
                if !equal { return false; }
            }
            _ => return false,
        }
    }
}

fn overlaps(left: &Path, right: &Path) -> bool { contains(left, right) || contains(right, left) }

pub(super) fn destination(project: &Project, selected: &Path, target: &Path) -> Result<PathBuf, String> {
    let selected = if selected.is_absolute() { selected.to_owned() } else { project.root.join(selected) };
    let name = selected.file_name().ok_or("请选择工程导出的子目录")?;
    relative(Path::new(name))?;
    let parent = checked(selected.parent().ok_or("导出目录缺少父目录")?, true)?;
    let destination = parent.join(name);
    if contains(&destination, &project.root) { return Err("导出目录不能覆盖工程根或其父目录".into()); }
    let data_root = project.context.editor_data.parent().ok_or("作者数据目录缺少父目录")?;
    if project.context.asset_roots.iter().any(|asset| overlaps(&destination, asset))
        || overlaps(&destination, data_root) || overlaps(&destination, target) {
        return Err("导出目录不能与资产根、作者数据或构建缓存相互包含".into());
    }
    // 源码/清单目录不能成为导出目标，即使调用时恰好为空也不改变其工程职责。
    for reserved in ["src", "crates", "tests", "examples", "benches", ".git", ".cargo", "target", "plugins", "editor_plugins"] {
        if overlaps(&destination, &project.root.join(reserved)) { return Err("导出目录不能占用工程源码、插件或构建目录".into()); }
    }
    match fs::symlink_metadata(&destination) {
        Ok(_) => { checked(&destination, true)?; }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
        Err(error) => return Err(format!("导出目录不可访问：{error}")),
    }
    Ok(destination)
}
