//! 固定品牌图片的宿主句柄；由每个编辑器App独立持有，不向游戏或工程配置发布。
use bevy::prelude::*;

#[derive(Resource)]
pub(crate) struct BrandImages {
    pub(crate) mark: Handle<Image>,
}
