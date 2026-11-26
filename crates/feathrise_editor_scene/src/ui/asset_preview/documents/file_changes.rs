//! 文件整理占用源路径时保留可读文档，成功回执把干净标签交接到实际磁盘地址。

use super::{resources::DocumentState, source::{is_project, project_key}, types::ViewMode};
use crate::{asset_catalog::document_format, resources::SceneEditorState};
use feathrise_editor_project_browser_model::file_operations::{FileOperationReceipt, FileSources, path_key};
use std::path::{Path, PathBuf};

pub(super) const FILE_CHANGE_PENDING: &str = "正在整理此文件，收到文件任务回执后可编辑";

impl DocumentState {
    pub(crate) fn protects_file_changes(&self, sources: &FileSources) -> bool {
        self.documents.iter().any(|doc| {
            (doc.dirty() || doc.owns_form(self.rules_form_input.as_ref()) || self.saving.as_deref() == Some(&doc.path)
                || self.saving_batch.iter().any(|key| key.path == doc.path) || self.json.edits_path(&doc.path))
                && sources.overlaps(&doc.disk.path)
        })
    }

    pub(super) fn file_change_pending(&self, path: &str) -> bool {
        if self.file_writes.is_empty() { return false; }
        self.documents.iter().find(|doc| doc.path == path).map_or_else(
            || self.physical_path(path).is_some_and(|path| self.file_writes.blocks(&path)),
            |doc| self.file_writes.blocks(&doc.disk.path),
        )
    }

    fn physical_path(&self, key: &str) -> Option<PathBuf> {
        if is_project(key) {
            self.project_source_path(key).map(|(root, relative)| root.join(relative))
        } else {
            Some(self.root.join(key))
        }
    }

    fn key_for_destination(&self, path: &Path) -> Option<String> {
        let key = path_key(path);
        let relative_to = |root: &Path| {
            let count = key.strip_prefix(path_key(root)).ok()?.components().count();
            let suffix: PathBuf = path.components().skip(path.components().count() - count).collect();
            suffix.to_str().map(|value| value.replace('\\', "/"))
        };
        if let Some(relative) = relative_to(&self.root) {
            return document_format(&relative).map(|_| relative);
        }
        let root = self.project_root.as_ref()?;
        let relative = relative_to(root)?;
        project_key(&self.root, root, &relative).ok().filter(|key| document_format(super::source::display_path(key)).is_some())
    }

    pub(crate) fn apply_file_receipt(&mut self, receipt: &FileOperationReceipt, mut editor: Option<&mut SceneEditorState>) {
        let rules_path = path_key(&self.root.join(feathrise_editor_documents_model::project_rules::PATH));
        let mut keys: Vec<_> = self.documents.iter().map(|doc| doc.path.clone())
            .chain(self.tabs().map(|tab| tab.path.clone())).chain(self.active.iter().cloned())
            .chain(self.sidebar_rules.path.iter().cloned()).collect();
        keys.sort_unstable();
        keys.dedup();
        for old in keys {
            let index = self.documents.iter().position(|doc| doc.path == old);
            if index.is_some_and(|index| self.documents[index].dirty()) { continue; }
            let path = index.map(|index| self.documents[index].disk.path.clone()).or_else(|| self.physical_path(&old));
            let Some((change, suffix)) = path.as_deref().and_then(|path| receipt.change_for(path)) else { continue; };
            if change.source_retained { continue; }
            let successor = change.destination.as_deref().and_then(|target| {
                let target = if suffix.as_os_str().is_empty() { target.to_owned() } else { target.join(&suffix) };
                let key = self.key_for_destination(&target)?;
                // 目标可能尚有来自外部修改的独立标签或草稿。它仍拥有该路径的会话。
                if key != old && (self.tabs().any(|tab| tab.path == key) || self.documents.iter().any(|doc| doc.path == key)) { return None; }
                let project_rules = path_key(&target) == rules_path;
                Some((key, target, project_rules))
            });
            if self.pending_load.as_ref().is_some_and(|request| request.path == old) { self.pending_load = None; }
            if self.sidebar_rules.path.as_deref() == Some(&old) {
                if let Some((key, _, true)) = successor.as_ref() {
                    self.sidebar_rules.path = Some(key.clone());
                } else {
                    self.sidebar_rules.path = None;
                    self.sidebar_rules.error = "工程规则文件已移动或删除，重新点击当前工具可打开工程规则。".into();
                }
                self.sidebar_rules.pending = None;
            }
            if let Some(index) = index {
                if let Some((key, path, project_rules)) = &successor {
                    let doc = &mut self.documents[index];
                    doc.path.clone_from(key);
                    doc.disk.path.clone_from(path);
                    // 目标可能刚成为主规则文件，也可能已移成子目录里的普通 JSON。
                    doc.disk.project_rules = *project_rules;
                    doc.generation = doc.generation.wrapping_add(1).max(1);
                    doc.format = document_format(super::source::display_path(key)).expect("已核对新文档格式");
                    doc.preview_revision = None;
                    doc.confirm_reload = false;
                    doc.external = Default::default();
                } else {
                    let retired = self.documents.remove(index);
                    if let Some(pool) = bevy::tasks::AsyncComputeTaskPool::try_get() {
                        pool.spawn(async move { drop(retired); }).detach();
                    }
                }
            }
            if let Some((key, _, _)) = &successor {
                for tab in self.expanded.iter_mut().chain(&mut self.parked).filter(|tab| tab.path == old) {
                    tab.path.clone_from(key);
                }
            } else {
                self.parked.retain(|tab| tab.path != old);
                if self.expanded.as_ref().is_some_and(|tab| tab.path == old) {
                    self.expanded = self.parked.pop();
                }
            }
            let new_key = successor.map(|(key, _, _)| key);
            if let Some(editor) = editor.as_deref_mut().filter(|editor| editor.selected_asset.as_ref() == Some(&old)) {
                editor.selected_asset.clone_from(&new_key);
            }
            if self.active.as_ref() == Some(&old) {
                self.active = new_key;
                self.load_ticket = self.load_ticket.wrapping_add(1).max(1);
                if index.is_none() && let Some(path) = self.active.clone() { self.request_load(path); }
            }
        }
    }

    pub(super) fn editing(&self) -> bool {
        self.current().is_some_and(|doc| doc.mode == ViewMode::Edit && !self.file_writes.blocks(&doc.disk.path))
    }
}
