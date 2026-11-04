//! 单槽后台工程目录扫描与最新结果采用；IO、筛选、排序和旧目录析构都不进入UI帧。

use bevy::{prelude::*, tasks::{IoTaskPool, futures::check_ready}};

use super::{resources::EditorProjectFileCatalog, types::{ScanRequest, ScanResult}};
use crate::asset_catalog::{scan::{scan_folder, FolderScope}, types::AssetCatalogFeedback};

pub(crate) fn start_project_file_scan(mut catalog: ResMut<EditorProjectFileCatalog>) {
    if catalog.task.is_some() { return; }
    let scan = catalog.active && catalog.is_loading();
    if !scan && catalog.retired_entries.is_none() && catalog.retired_source.is_none() { return; }
    let request = scan.then(|| {
        let (context, root) = catalog.source.as_ref().expect("is_loading要求明确工程来源");
        ScanRequest {
            context: *context, project_root: root.clone().expect("is_loading要求工程根"),
            revision: catalog.requested_revision, folder: catalog.requested_folder.clone(),
            filter: catalog.filter.clone(),
        }
    });
    let retired = catalog.retired_entries.take();
    let retired_source = catalog.retired_source.take();
    catalog.task = Some(IoTaskPool::get().spawn(async move {
        drop(retired);
        drop(retired_source);
        request.map(scan_request)
    }));
}

pub(super) fn scan_request(request: ScanRequest) -> ScanResult {
    let result = scan_folder(request.project_root.clone(), request.folder.clone(), FolderScope::ProjectFiles)
        .map(|mut entries| {
            // 目录始终保留，筛选后的父层仍能进入src/crates；只遍历当前后台完整结果。
            let filter = request.filter.to_lowercase();
            if !filter.is_empty() {
                entries.retain(|entry| entry.is_directory || entry.display_name.to_lowercase().contains(&filter));
            }
            entries
        });
    ScanResult { request, result }
}

pub(crate) fn poll_project_file_scan(mut catalog: ResMut<EditorProjectFileCatalog>) {
    let Some(task) = catalog.task.as_mut() else { return; };
    let Some(completed) = check_ready(task) else { return; };
    catalog.task = None;
    if let Some(completed) = completed { adopt(&mut catalog, completed); }
}

pub(super) fn adopt(catalog: &mut EditorProjectFileCatalog, completed: ScanResult) {
    let request = completed.request;
    let current = catalog.source.as_ref().is_some_and(|(context, root)|
        *context == request.context && root.as_ref() == Some(&request.project_root))
        && catalog.requested_revision == request.revision
        && catalog.requested_folder == request.folder && catalog.filter == request.filter;
    if !current {
        if let Ok(entries) = completed.result {
            debug_assert!(catalog.retired_entries.is_none());
            catalog.retired_entries = Some(entries);
        }
        return;
    }
    catalog.applied_revision = request.revision;
    match completed.result {
        Ok(entries) => {
            debug_assert!(catalog.retired_entries.is_none());
            catalog.retired_entries = Some(std::mem::replace(&mut catalog.entries, entries));
            catalog.current_folder = request.folder;
            catalog.feedback = Some(AssetCatalogFeedback { revision: request.revision, result: Ok(()) });
        }
        Err(error) => {
            catalog.feedback = Some(AssetCatalogFeedback { revision: request.revision, result: Err(error) });
        }
    }
}
