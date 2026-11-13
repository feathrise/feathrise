//! 类型菜单、片段列表与共享服务挂载的短期意图，不把 GLB 片段当作磁盘文件。

use bevy::prelude::*;
use feathrise_editor_project_browser_model::AssetTypeFilter;

pub(super) const FILTERS: [AssetTypeFilter; 9] = [
    AssetTypeFilter::All,
    AssetTypeFilter::Asset,
    AssetTypeFilter::Glb,
    AssetTypeFilter::Model,
    AssetTypeFilter::Animation,
    AssetTypeFilter::Skeleton,
    AssetTypeFilter::Image,
    AssetTypeFilter::Audio,
    AssetTypeFilter::Video,
];

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum BrowserAction {
    Filter,
    View,
    Search,
    Missing,
    Favorites,
    Recent,
    Dismiss,
}

#[derive(Clone)]
pub(super) enum PopupKind {
    Filters,
    Clips { path: String, revision: u64 },
}
#[derive(Clone)]
pub(super) struct BrowserPopup {
    pub window: u64,
    pub anchor: Entity,
    pub kind: PopupKind,
    pub first: usize,
}
pub(super) struct ServiceHost {
    pub window: u64,
    pub search: bool,
    pub recent: bool,
}

impl ServiceHost {
    /// 收藏与最近访问同用导航面板，但只有重按当前分区才收起它。
    pub(super) fn shows_shortcuts(&self, window: u64, recent: bool) -> bool {
        self.window == window && !self.search && self.recent == recent
    }
}

pub(super) fn type_label(filter: AssetTypeFilter, english: bool) -> &'static str {
    match (filter, english) {
        (AssetTypeFilter::All, false) => "全部",
        (AssetTypeFilter::All, true) => "All",
        (AssetTypeFilter::Asset, false) => "资产",
        (AssetTypeFilter::Asset, true) => "Assets",
        (AssetTypeFilter::Glb, _) => "GLB",
        (AssetTypeFilter::Model, false) => "模型",
        (AssetTypeFilter::Model, true) => "Models",
        (AssetTypeFilter::Animation, false) => "动画",
        (AssetTypeFilter::Animation, true) => "Animation",
        (AssetTypeFilter::Skeleton, false) => "骨架",
        (AssetTypeFilter::Skeleton, true) => "Skeletons",
        (AssetTypeFilter::Image, false) => "图片",
        (AssetTypeFilter::Image, true) => "Images",
        (AssetTypeFilter::Audio, false) => "音频",
        (AssetTypeFilter::Audio, true) => "Audio",
        (AssetTypeFilter::Video, false) => "视频",
        (AssetTypeFilter::Video, true) => "Video",
    }
}
