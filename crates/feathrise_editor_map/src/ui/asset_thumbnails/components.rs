//! 资产缩略图的固定UI槽位、专用实体与跨渲染帧回执。

use bevy::prelude::*;
use bevy::render::extract_component::ExtractComponent;
use feathrise_editor_core::AssetBrowserViewMode;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU8},
};

#[derive(Component, Clone, Default)]
pub(super) struct ViewModeButton(pub AssetBrowserViewMode);
#[derive(Component)]
pub(super) struct ThumbnailImage(pub usize);
#[derive(Component)]
pub(super) struct ThumbnailPlaceholder(pub usize);
#[derive(Component)]
pub(super) struct ThumbnailStatus(pub usize);
#[derive(Component)]
pub(super) struct ThumbnailIcon {
    pub slot: usize,
    pub part: u8,
}
#[derive(Component)]
pub(super) struct ThumbnailCamera;
#[derive(Component)]
pub(super) struct ThumbnailRoot;

#[derive(Default)]
pub(super) struct RenderReceipt {
    pub ready: AtomicBool,
    pub failed: AtomicBool,
    pub frames: AtomicU8,
}

/// 每个网格与任务持有同一票据，旧RenderApp帧无法确认下一张快照。
#[derive(Component, Clone, ExtractComponent)]
#[extract_app(bevy::render::RenderApp)]
pub(super) struct ThumbnailProbe {
    pub camera: Entity,
    pub target: Handle<Image>,
    pub mesh: Handle<Mesh>,
    pub material: Handle<StandardMaterial>,
    pub receipt: Arc<RenderReceipt>,
}
