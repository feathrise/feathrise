//! 正式根上的精确删除遮蔽；保存确认保留旧根责任直到退役。

use std::sync::{Weak, atomic::AtomicBool};
use bevy::prelude::{Component, Visibility};
use feathrise_map_content::MapId;

/// 删除与作者预览共用底层显隐，但分别拥有恢复责任。
#[derive(Component, Debug)]
pub(crate) struct PlacementDeletionSuppression {
    pub(crate) original_visibility: Visibility,
    pub(super) map: MapId,
    pub(super) id: String,
    pub(super) ticket: Weak<()>,
    pub(super) scope: Weak<AtomicBool>,
    pub(super) committed: bool,
}
