//! 工程源码与资产文本共用文档会话；内部命名空间只作身份，磁盘操作始终使用显式受检根。

use std::path::{Component, Path, PathBuf};

use super::resources::DocumentState;

const PROJECT_PREFIX: &str = "project://";

#[derive(Clone)]
pub(super) struct SourceRoots {
    pub assets: PathBuf,
    pub project: Option<PathBuf>,
}

pub(crate) fn display_path(path: &str) -> &str { path.strip_prefix(PROJECT_PREFIX).unwrap_or(path) }
pub(crate) fn is_project(path: &str) -> bool { path.starts_with(PROJECT_PREFIX) }

fn valid_relative(path: &str) -> bool {
    !path.is_empty() && path.len() <= 4096 && !path.contains(['\\', ':'])
        && Path::new(path).components().all(|part| matches!(part, Component::Normal(_)))
}

/// 工程内的资产文本继续使用原身份；同一文件从两个视图打开不能产生两份可写草稿。
pub(super) fn project_key(assets: &Path, project: &Path, relative: &str) -> Result<String, String> {
    if !project.is_absolute() || !valid_relative(relative) {
        return Err("工程文件必须使用当前工程根内的相对路径".into());
    }
    if let Ok(asset_prefix) = assets.strip_prefix(project) {
        if let Ok(path) = Path::new(relative).strip_prefix(asset_prefix) {
            let path = path.to_str().ok_or("文件路径不是有效Unicode")?.replace('\\', "/");
            if valid_relative(&path) { return Ok(path); }
        }
    }
    Ok(format!("{PROJECT_PREFIX}{relative}"))
}

impl SourceRoots {
    /// 仅由后台IO调用；复核工程与资产根的真实包含关系，目录链接改变后不越界写盘。
    pub fn resolve<'a>(&self, key: &'a str) -> Result<(&Path, &'a str), String> {
        if let Some(relative) = key.strip_prefix(PROJECT_PREFIX) {
            if !valid_relative(relative) { return Err("工程文档路径无效".into()); }
            return Ok((self.project.as_deref().ok_or("工程文档尚未绑定工程根")?, relative));
        }
        if let Some(project) = &self.project {
            let project = project.canonicalize().map_err(|error| format!("无法核验工程目录：{error}"))?;
            let assets = self.assets.canonicalize().map_err(|error| format!("无法核验资产目录：{error}"))?;
            if !assets.starts_with(project) { return Err("文档资产根已离开当前工程，内容已保留".into()); }
        }
        Ok((&self.assets, key))
    }
}

impl DocumentState {
    pub(super) fn source_roots(&self) -> SourceRoots {
        SourceRoots { assets: self.root.clone(), project: self.project_root.clone() }
    }

    pub(crate) fn project_source_path<'a>(&self, key: &'a str) -> Option<(&Path, &'a str)> {
        Some((self.project_root.as_deref()?, key.strip_prefix(PROJECT_PREFIX)?))
    }

    /// 与资产菜单相同，只复制词法地址；内部project://身份不应出现在剪贴板或IDE参数中。
    pub(crate) fn project_path_to_copy(&self, key: &str, absolute: bool) -> Result<String, String> {
        let (root, relative) = self.project_source_path(key).ok_or("工程文档根尚未绑定")?;
        if !root.is_absolute() || !valid_relative(relative) { return Err("工程文档路径无效".into()); }
        if !absolute { return Ok(relative.to_owned()); }
        crate::ui::external_files::platform_path(&root.join(relative)).into_os_string().into_string()
            .map_err(|_| "工程文件路径包含无法复制的字符".into())
    }

    pub(super) fn bind_project_source(&mut self, root: &Path, assets: Option<&Path>) -> Result<(), String> {
        if self.project_root.as_deref().is_some_and(|current| current != root) {
            return Err("文件会话属于另一工程，请先正常关闭旧工程".into());
        }
        if let Some(assets) = assets.filter(|assets| *assets != self.root.as_path()) {
            if !self.documents.is_empty() || self.expanded() || self.io.is_some() || self.pending_load.is_some() {
                return Err("现有文档会话尚未收口，不能更换工程文件来源".into());
            }
            self.root = assets.to_path_buf();
        }
        self.project_root = Some(root.to_path_buf());
        Ok(())
    }
}

#[cfg(test)]
mod tests;
