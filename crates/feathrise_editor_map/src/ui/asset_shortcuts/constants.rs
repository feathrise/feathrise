//! 个人导航记录和固定行池预算；不限制资产目录本身的规模。

pub(super) const FAVORITES: usize = 64;
pub(super) const RECENT: usize = 20;
pub(super) const PATH_BYTES: usize = 1024;
pub(super) const STATE_BYTES: u64 = 128 * 1024;
pub(super) const PENDING_CHANGES: usize = 256;
pub(super) const ROW_HEIGHT: f32 = 24.0;
pub(super) const ROW_GAP: f32 = 2.0;
pub(super) const LIST_HEIGHT: f32 = 154.0;
pub(super) const STATE_FILE: &str = "editor_asset_navigation.json";
