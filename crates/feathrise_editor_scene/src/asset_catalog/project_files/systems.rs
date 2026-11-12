//! 单槽后台工程目录扫描与最新结果采用；IO、筛选、排序和旧目录析构都不进入UI帧。

use bevy::{prelude::*, tasks::{IoTaskPool, futures::check_ready}};

use feathrise_editor_project_browser_model::{DirectoryOperations, DirectoryQuery, DirectoryWorker, FolderScope};
use super::{resources::EditorProjectFileCatalog, types::{ScanRequest, ScanResult}};
use crate::asset_catalog::{
    filter::{DirectoryFilters, DirectorySnapshot},
    types::AssetCatalogFeedback,
};

pub(crate) fn start_project_file_scan(operations: Res<DirectoryOperations>, mut catalog: ResMut<EditorProjectFileCatalog>) {
    if catalog.task.is_some() { return; }
    if catalog.active && catalog.project_root().is_some() && catalog.worker.take_changed() && !catalog.is_loading() {
        catalog.requested_revision = catalog.requested_revision.wrapping_add(1);
    }
    let scan = catalog.active && catalog.is_loading();
    let stop_watching = catalog.project_root().is_none() && catalog.worker.is_watching();
    if !scan && !stop_watching && catalog.retired_entries.is_none() && catalog.retired_source.is_none() { return; }
    let request = scan.then(|| {
        let (context, root) = catalog.source.as_ref().expect("is_loading要求明确工程来源");
        ScanRequest {
            context: *context, project_root: root.clone().expect("is_loading要求工程根"),
            asset_root: catalog.asset_root.clone(),
            revision: catalog.requested_revision, folder: catalog.requested_folder.clone(),
            filters: DirectoryFilters { filename: catalog.filter.clone(), kind: catalog.type_filter },
        }
    });
    let retired = catalog.retired_entries.take();
    let retired_source = catalog.retired_source.take();
    if scan { catalog.worker.ensure(*operations); }
    let worker = catalog.worker.clone();
    catalog.task = Some(IoTaskPool::get().spawn(async move {
        drop(retired);
        drop(retired_source);
        if stop_watching { worker.clear(); }
        request.map(|request| scan_request_with_worker(request, &worker))
    }));
}

#[cfg(test)]
pub(super) fn scan_request(request: ScanRequest) -> ScanResult {
    scan_request_with_worker(request, &super::super::scan::test_directory_worker())
}

fn scan_request_with_worker(request: ScanRequest, worker: &DirectoryWorker) -> ScanResult {
    let result = worker.scan(request.project_root.clone(), request.folder.clone(), DirectoryQuery {
        context: request.context, scope: FolderScope::ProjectFiles, asset_root: request.asset_root.as_deref(), filters: &request.filters,
        reveal_file: None,
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
        && catalog.requested_folder == request.folder && catalog.filter == request.filters.filename
        && catalog.type_filter == request.filters.kind && catalog.asset_root == request.asset_root;
    if !current {
        if let Ok(entries) = completed.result {
            debug_assert!(catalog.retired_entries.is_none());
            catalog.retired_entries = Some(entries);
        }
        return;
    }
    catalog.applied_revision = request.revision;
    match completed.result {
        Ok(snapshot) => {
            debug_assert!(catalog.retired_entries.is_none());
            catalog.retired_entries = Some(DirectorySnapshot {
                entries: std::mem::replace(&mut catalog.entries, snapshot.entries),
                ..Default::default()
            });
            catalog.adopted_type_filter = request.filters.kind;
            catalog.current_folder = request.folder;
            catalog.feedback = Some(AssetCatalogFeedback { revision: request.revision, result: Ok(()) });
        }
        Err(error) => {
            // 失败仍显示最后采用的目录；后续刷新/筛选必须从该目录恢复，不能反复扫描失败目标。
            catalog.requested_folder = catalog.current_folder.clone();
            catalog.feedback = Some(AssetCatalogFeedback { revision: request.revision, result: Err(error) });
        }
    }
}
