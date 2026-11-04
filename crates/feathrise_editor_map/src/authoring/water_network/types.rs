//! 多网格水网异步新增点的轻量冻结身份；不持有水域文档或地形像素。

use crate::authoring::terrain_ground::CursorIdentity;
use feathrise_map_content::{MapId, TerrainId};

pub(crate) struct ObjectWaterPointClick {
    pub(super) identity: CursorIdentity,
    pub(super) authority: u64,
    pub(super) map: MapId,
    pub(super) terrain: TerrainId,
    pub(super) feature: String,
    pub(super) waterfall: bool,
}
