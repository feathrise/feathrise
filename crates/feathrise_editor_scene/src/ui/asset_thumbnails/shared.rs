//! 文件桌面的有界需求入口；所有文件窗与固定栏共用原64张缓存、单IO检查和唯一串行相机。

use super::{resources::ThumbnailState, types::ThumbnailResult};
use bevy::{ecs::system::SystemParam, prelude::*};
use feathrise_scene_content::SceneAssetKind;

#[derive(Resource, Default)]
pub(crate) struct SharedThumbnailDemand {
    pub(crate) revision: u64,
    pub(crate) items: Vec<(String, SceneAssetKind)>,
}

impl SharedThumbnailDemand {
    pub(crate) const CAPACITY: usize = super::constants::CACHE_CAPACITY;

    pub(crate) fn replace(&mut self, mut items: Vec<(String, SceneAssetKind)>) {
        // 大窗口可以显示更多条目，排入后台的不同缩略图仍受原缓存预算约束。
        items.truncate(Self::CAPACITY);
        if self.items != items {
            self.items = items;
            self.revision = self.revision.wrapping_add(1);
        }
    }
}

#[derive(SystemParam)]
pub(crate) struct SharedThumbnails<'w> {
    state: Res<'w, ThumbnailState>,
}

impl SharedThumbnails<'_> {
    pub(crate) fn image(&self, path: &str) -> Option<Handle<Image>> {
        match &self.state.cache.get(path)?.result {
            ThumbnailResult::Ready(image) => Some(image.clone()),
            _ => None,
        }
    }
}
