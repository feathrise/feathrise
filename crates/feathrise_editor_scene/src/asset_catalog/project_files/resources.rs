//! 工程文件会话的导航与选择；只有完整扫描成功才替换可见目录，失败继续保留原内容。

use std::path::{Path, PathBuf};

use bevy::{prelude::Resource, tasks::Task};

use super::types::{ProjectFileIdentity, ProjectFileNavigation, ScanResult};
use crate::{asset_catalog::{scan::hidden_project_entry, types::AssetCatalogFeedback}, types::EditorAssetEntry};

#[derive(Resource, Default)]
pub(crate) struct EditorProjectFileCatalog {
    /// 只在工程文件视图可见时启动新扫描；已在途任务仍保留槽位至真正完成。
    pub(crate) active: bool,
    /// 公开目录始终属于最后成功快照；请求失败不能把旧条目标成另一个目录。
    pub(crate) current_folder: String,
    pub(crate) entries: Vec<EditorAssetEntry>,
    pub(crate) filter: String,
    pub(crate) selected_file: Option<ProjectFileIdentity>,
    pub(crate) feedback: Option<AssetCatalogFeedback>,
    pub(crate) requested_revision: u64,
    pub(crate) applied_revision: u64,
    pub(super) source: Option<(u64, Option<PathBuf>)>,
    /// 最新待处理导航与筛选一起冻结，连续输入只替换这一份意图。
    pub(super) requested_folder: String,
    pub(super) task: Option<Task<Option<ScanResult>>>,
    /// 完成值或已显示目录各最多一份退役预算，O(1)移交同一IO槽析构。
    pub(super) retired_entries: Option<Vec<EditorAssetEntry>>,
    pub(super) retired_source: Option<Vec<EditorAssetEntry>>,
}

impl EditorProjectFileCatalog {
    /// 宿主显式绑定工程根；不从assets父目录猜测，也不在主帧访问文件系统。
    pub(crate) fn bind_project_source(&mut self, context: u64, root: Option<PathBuf>) -> bool {
        if self.source.as_ref().is_some_and(|source| source.0 == context && source.1 == root) {
            return false;
        }
        self.source = Some((context, root));
        self.requested_revision = self.requested_revision.wrapping_add(1);
        self.requested_folder.clear();
        self.current_folder.clear();
        self.filter.clear();
        self.selected_file = None;
        self.feedback = None;
        if !self.entries.is_empty() {
            debug_assert!(self.retired_source.is_none());
            self.retired_source = Some(std::mem::take(&mut self.entries));
        }
        true
    }

    pub(crate) fn project_root(&self) -> Option<&Path> {
        self.source.as_ref().and_then(|(_, root)| root.as_deref())
    }

    pub(crate) fn source_context(&self) -> Option<u64> { self.source.as_ref().map(|source| source.0) }

    pub(crate) fn is_loading(&self) -> bool {
        self.project_root().is_some() && self.requested_revision != self.applied_revision
    }

    pub(crate) fn request_folder(&mut self, folder: &str) -> Result<(), String> {
        if self.project_root().is_none() { return Err("请先打开工程后浏览工程文件".into()); }
        self.requested_folder = normalize_folder(folder)?;
        self.filter.clear();
        self.requested_revision = self.requested_revision.wrapping_add(1);
        self.feedback = None;
        Ok(())
    }

    pub(crate) fn set_filter(&mut self, filter: &str) {
        if self.filter == filter { return; }
        self.filter = filter.to_owned();
        self.feedback = None;
        self.requested_revision = self.requested_revision.wrapping_add(1);
    }

    pub(crate) fn navigate(&mut self, action: ProjectFileNavigation) {
        match action {
            ProjectFileNavigation::Refresh => {
                self.requested_revision = self.requested_revision.wrapping_add(1);
                self.feedback = None;
            }
            ProjectFileNavigation::Up => {
                let parent = self.current_folder.rsplit_once('/').map_or("", |(parent, _)| parent).to_owned();
                let _ = self.request_folder(&parent);
            }
        }
    }

    /// 固定行只传当前完整快照的索引；目录请求不改变文件选择，更不武装场景摆放。
    pub(crate) fn select_entry(&mut self, index: usize) -> Option<ProjectFileIdentity> {
        let entry = self.entries.get(index)?.clone();
        if entry.is_directory {
            let _ = self.request_folder(&entry.asset_path);
            return None;
        }
        let (context, Some(project_root)) = self.source.as_ref()? else { return None; };
        let identity = ProjectFileIdentity {
            context: *context, project_root: project_root.clone(), relative_path: entry.asset_path,
        };
        self.selected_file = Some(identity.clone());
        Some(identity)
    }
}

fn normalize_folder(folder: &str) -> Result<String, String> {
    let normalized = folder.replace('\\', "/");
    if normalized.starts_with('/') || normalized.contains(':') {
        return Err("工程目录必须是工程根内的相对路径".into());
    }
    let parts = normalized.split('/').filter(|part| !part.is_empty()).collect::<Vec<_>>();
    if parts.iter().any(|part| matches!(*part, "." | "..") || hidden_project_entry(part)) {
        return Err("工程目录不能跳出工程根或进入隐藏的构建/Git/回收目录".into());
    }
    Ok(parts.join("/"))
}
