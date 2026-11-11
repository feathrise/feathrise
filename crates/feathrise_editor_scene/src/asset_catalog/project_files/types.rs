//! 工程文件选择与后台目录快照的完整来源身份，禁止使用资产相对路径冒充工程路径。

use std::path::PathBuf;

use crate::asset_catalog::filter::{DirectoryFilters, DirectorySnapshot};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProjectFileIdentity {
    pub(crate) context: u64,
    pub(crate) project_root: PathBuf,
    pub(crate) relative_path: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProjectFileNavigation { Refresh, Up }

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ScanRequest {
    pub(super) context: u64,
    pub(super) project_root: PathBuf,
    pub(super) asset_root: Option<PathBuf>,
    pub(super) revision: u64,
    pub(super) folder: String,
    pub(super) filters: DirectoryFilters,
}

pub(super) struct ScanResult {
    pub(super) request: ScanRequest,
    pub(super) result: Result<DirectorySnapshot, String>,
}
