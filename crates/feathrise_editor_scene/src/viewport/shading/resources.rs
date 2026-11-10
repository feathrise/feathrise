//! 视口着色偏好随编辑器会话保留，独立于场景作者历史。
use super::types::ShadingMode;
use bevy::prelude::*;

#[derive(Resource)]
pub(crate) struct ShadingState {
    pub(crate) mode: ShadingMode,
    pub(super) menu_open: bool,
    pub(super) xray: bool,
    pub(super) cavity: bool,
    pub(super) normals: bool,
    pub(super) studio: bool,
    pub(super) background: bool,
}
impl Default for ShadingState {
    fn default() -> Self {
        Self {
            mode: ShadingMode::Rendered,
            menu_open: false,
            // Blender式线框默认观察完整拓扑，包括地面以下和背面的边；可手动关闭穿透。
            xray: true,
            cavity: true,
            normals: false,
            studio: true,
            background: true,
        }
    }
}
