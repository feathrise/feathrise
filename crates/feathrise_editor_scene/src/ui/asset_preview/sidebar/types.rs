//! 资产页固定字段及小型表现快照；不缓存目录条目或文档正文。

use crate::asset_catalog::AssetCatalogFeedback;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum SidebarField {
    #[default]
    DirectoryFeedback,
    Name,
    Kind,
    Path,
    CopyFeedback,
}

#[derive(Default)]
pub(super) struct SidebarObservation {
    pub(super) initialized: bool,
    pub(super) folder: String,
    pub(super) requested: u64,
    pub(super) applied: u64,
    pub(super) count: usize,
    pub(super) feedback: Option<AssetCatalogFeedback>,
    pub(super) selected: Option<String>,
    pub(super) copied: String,
}
