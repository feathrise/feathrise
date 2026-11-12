//! 场景定位沿用当前工程身份和采用回执，后台目录扫描由工程浏览工作器执行。

use super::types::{AssetScanContents, AssetScanRequest, AssetScanResult};
use feathrise_editor_project_browser_model::{DirectoryQuery, DirectorySnapshot, DirectoryWorker};
pub(super) use feathrise_editor_project_browser_model::{FolderScope, hidden_project_entry};
use std::path::PathBuf;

#[cfg(test)]
use feathrise_editor_project_browser_model::{AssetTypeFilter, DirectoryFilters, EditorAssetEntry};

#[cfg(test)]
pub(super) fn test_directory_worker() -> DirectoryWorker {
    let mut worker = DirectoryWorker::default();
    worker.ensure(feathrise_editor_project_browser::directory_operations());
    worker
}

#[cfg(test)]
fn scan_directory(root: PathBuf, folder: String, query: DirectoryQuery<'_>, worker: &DirectoryWorker) -> Result<DirectorySnapshot, String> {
    worker.scan(root, folder, query)
}

/// 扫描当前一层目录，目录优先、名称不区分大小写排序。
#[cfg(test)]
pub(super) fn scan_asset_folder(
    asset_root: PathBuf,
    current_folder: String,
) -> Result<Vec<EditorAssetEntry>, String> {
    scan_folder(asset_root, current_folder, FolderScope::Assets)
}

#[cfg(test)]
pub(super) fn scan_folder(
    asset_root: PathBuf,
    current_folder: String,
    scope: FolderScope,
) -> Result<Vec<EditorAssetEntry>, String> {
    scan_directory(asset_root, current_folder, DirectoryQuery {
        context: 0, scope, asset_root: None, filters: &DirectoryFilters::default(), reveal_file: None,
    }, &test_directory_worker()).map(|snapshot| snapshot.entries)
}

/// 唯一 IO 任务先完整扫描排序，再在后台精确定位文件；主线程不再线性搜索整表。
pub(super) fn scan_asset_request(
    asset_root: PathBuf,       // 本工作树的唯一资产根，规范化和越界校验在后台执行。
    request: AssetScanRequest, // 所有可影响采用的输入身份都随请求冻结。
    worker: &DirectoryWorker,
) -> AssetScanResult {
    let snapshot = worker.scan(asset_root, request.folder.clone(), DirectoryQuery {
        context: request.context, scope: FolderScope::Assets, asset_root: None, filters: &request.filters,
        reveal_file: request.reveal_file.as_deref(),
    });
    finish_asset_snapshot(request, snapshot)
}

/// 对完整排序结果计算可见页所需索引；测试可以注入纯条目，生产只从 IO 任务调用。
#[cfg(test)]
pub(super) fn finish_asset_scan(
    request: AssetScanRequest, // 冻结的父目录、全文件路径和修订。
    entries: Result<Vec<EditorAssetEntry>, String>, // 失败保持具体 IO 诊断，不伪造空目录成功。
) -> AssetScanResult {
    finish_asset_snapshot(request, entries.map(|entries| DirectorySnapshot { entries, ..Default::default() }))
}

fn finish_asset_snapshot(request: AssetScanRequest, snapshot: Result<DirectorySnapshot, String>) -> AssetScanResult {
    let result = snapshot.and_then(|snapshot| {
        let reveal_index = if let Some(file_path) = request.reveal_file.as_deref() {
            Some(
                snapshot.entries
                    .iter()
                    .position(|entry| !entry.is_directory && entry.asset_path == file_path)
                    .ok_or_else(|| {
                        format!(
                            "在 assets/{} 中找不到文件 assets/{file_path}",
                            request.folder
                        )
                    })?,
            ) // 必须完整路径一致，不能匹配别处同名文件。
        } else {
            None // 普通浏览不需要全表定位。
        };
        Ok(AssetScanContents {
            snapshot,    // 目录条目与片段索引一起原子采用。
            reveal_index, // 页码由现有十二行容量在采用点计算。
        })
    });
    AssetScanResult { request, result } // 完成值保留全部输入身份供主线程再次核对。
}

#[cfg(test)]
mod io_tests; // 超过旧目录上限与根外路径均在独立临时夹具中验证。
