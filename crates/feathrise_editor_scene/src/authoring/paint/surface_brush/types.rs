//! 地表等待起笔的固定身份；只保存弱地面票据和小型元数据，不持有或复制作者画布。

use crate::authoring::terrain_ground::CursorIdentity;
use feathrise_scene_content::TerrainId;

pub(super) enum SurfaceSampleProgress {
    Waiting,
    Applied(Option<crate::authoring::surface_layer_edit::PendingSurfaceBrushSample>),
}

#[derive(Clone, PartialEq)]
pub(crate) struct ObjectSurfacePress {
    pub(super) cursor: CursorIdentity,
    pub(super) input_authority: u64,
    pub(super) input_epoch: u64,
    pub(super) terrain: TerrainId,
    pub(super) surface_revision: u64,
    pub(super) layer: Option<(usize, String)>,
    pub(super) preset: Option<String>,
    pub(super) erase: bool,
}
