//! 按压期间冻结地形ID与整平高度，松手后下一刷印不能继续合并旧历史。
use bevy::prelude::*;
use feathrise_map_content::{MapId, TerrainId};
#[derive(Resource, Default)]
pub(super) struct BrushGesture {
    pub serial: u64,
    pub active: Option<(MapId, TerrainId, u64, Vec3)>,
    /// 首次按下冻结输入票据和工具；失焦/切模式后即使还按住，也不能复活旧雕刻授权。
    pub input_authority: Option<u64>,
    pub tool: Option<crate::types::EditorTool>,
}
