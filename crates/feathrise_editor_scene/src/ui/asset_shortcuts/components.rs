//! 固定收藏/最近访问行槽；绑定完整路径防止列表重排后误点另一文件。

use super::types::{Section, ShortcutTarget};
use bevy::prelude::*;

#[derive(Component, Clone, Default)]
pub(crate) struct ShortcutRoot;

#[derive(Component, Clone, Default)]
pub(crate) struct ShortcutList(pub(super) Section);
#[derive(Component, Clone, Default)]
pub(super) struct ShortcutCount(pub(super) Section);
#[derive(Component, Clone, Default)]
pub(super) struct ShortcutPanel(pub(super) Section);
#[derive(Component, Clone, Default)]
pub(super) struct ShortcutFeedback;
#[derive(Component, Clone, Default)]
pub(super) struct ShortcutEmpty(pub(super) Section);
#[derive(Component)]
pub(super) struct ShortcutRow {
    pub(super) section: Section,
    pub(super) slot: usize,
}
#[derive(Component)]
pub(super) struct ShortcutLabel;
#[derive(Component, Default)]
pub(super) struct ShortcutBinding(pub(super) Option<ShortcutTarget>);

#[derive(Component, Clone, Default)]
pub(super) enum ShortcutAction {
    #[default]
    Refresh,
    Collapse(Section),
    ClearRecent,
    Locate(Section),
    Remove(Section),
}
