//! 项目与资产目录共用同一套受检扫描、元数据缓存和文件监听。

use bevy::prelude::*;
use feathrise_editor_documents_model::document_format;
use feathrise_editor_project_browser_model::{
    DirectoryBackend, DirectoryOperations, DirectoryQuery, DirectorySnapshot,
};
use std::{path::PathBuf, sync::Arc};

mod filter;
mod file_operations;
mod scan;
mod watch;

pub struct ProjectBrowserPlugin;

impl Plugin for ProjectBrowserPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(directory_operations())
            .insert_resource(file_operation_service());
    }
}

/// 文件窗口和旧工作台共用同一后台服务；工厂只分配队列，首个显式请求才启动IO任务。
pub fn file_operation_service() -> feathrise_editor_project_browser_model::file_operations::FileOperationService {
    file_operations::service()
}

pub fn directory_operations() -> DirectoryOperations {
    DirectoryOperations {
        create: || Arc::new(watch::DirectoryWorker::default()),
    }
}

impl DirectoryBackend for watch::DirectoryWorker {
    fn take_changed(&self) -> bool {
        self.take_changed()
    }
    fn request_refresh(&self) {
        self.request_refresh();
    }
    fn is_watching(&self) -> bool {
        self.is_watching()
    }
    fn clear(&self) {
        self.clear();
    }

    fn scan(
        &self,
        root: PathBuf,
        folder: String,
        query: DirectoryQuery<'_>,
    ) -> Result<DirectorySnapshot, String> {
        scan::scan_directory(root, folder, query, self)
    }
}
