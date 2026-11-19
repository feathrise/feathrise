//! 文件窗浏览工具、锚定菜单和唯一原服务面板的挂载点。

use super::types::BrowserAction;
use bevy::prelude::*;
use feathrise_editor_project_browser_model::AssetTypeFilter;

#[derive(Component)]
pub(crate) struct BrowserButton {
    pub(crate) window: u64,
    pub(crate) action: BrowserAction,
}
#[derive(Component)]
pub(super) struct BrowserButtonLabel(pub BrowserAction);
#[derive(Component)]
pub(super) struct BrowserToolIcon(pub &'static str);
#[derive(Component)]
pub(crate) struct BrowserOverlay {
    pub(crate) window: u64,
    pub(crate) service: bool,
}
#[derive(Component)]
pub(crate) struct BrowserChoice {
    pub(crate) window: u64,
    pub(crate) slot: usize,
    pub(crate) filter: Option<AssetTypeFilter>,
    pub(crate) clip: Option<usize>,
}
#[derive(Component)]
pub(super) struct ChoiceLabel;
#[derive(Component)]
pub(crate) struct ClipExpander {
    pub(crate) row: Entity,
}
#[derive(Component)]
pub(crate) struct RowHint;
#[derive(Component)]
pub(super) struct ServiceOrigin(pub Entity);

#[derive(Message)]
pub(crate) enum BrowserNavigate {
    InWindow { window: u64, path: String, directory: bool },
    RevealFile { context: u64, path: String },
}
