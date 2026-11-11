//! 单一工程目录跨资产根时的待采用身份；不公开内部来源切换为用户界面选项。

use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Roots {
    pub(super) context: u64,
    pub(super) project: PathBuf,
    pub(super) assets: PathBuf,
}

pub(crate) struct PendingDirectory {
    pub(super) roots: Roots,
    pub(super) project_revision: u64,
    pub(super) project_folder: String,
    pub(super) asset_folder: String,
    pub(super) asset_revision: u64,
}

#[derive(Default)]
pub(crate) struct NavigationState {
    pub(super) source: Option<Roots>,
    pub(super) pending: Option<PendingDirectory>,
    pub(super) inspected: Option<(Roots, u64)>,
    pub(super) observed_pending: Option<u64>,
    pub(super) observed_completed: Option<u64>,
}
