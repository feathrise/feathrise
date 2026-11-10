//! 视口着色偏好随编辑器会话保留，独立于地图作者历史。
use super::{
    components::ShadingView,
    types::{ShadingAction, ShadingMode},
};
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

/// 中央 GLB 和地图各保留一份会话偏好，切换文件或标签只改变应用目标。
#[derive(Resource, Default)]
pub(crate) struct AssetPreviewShadingState(pub(crate) ShadingState);

impl ShadingState {
    pub(crate) fn apply(&mut self, action: ShadingAction) {
        match action {
            ShadingAction::Mode(mode) => {
                self.mode = mode;
                self.menu_open = false;
            }
            ShadingAction::Menu => self.menu_open = !self.menu_open,
            ShadingAction::Xray => self.xray = !self.xray,
            ShadingAction::Cavity => self.cavity = !self.cavity,
            ShadingAction::Normals => self.normals = !self.normals,
            ShadingAction::Studio => self.studio = !self.studio,
            ShadingAction::Background => self.background = !self.background,
        }
    }

    pub(crate) fn dismiss(&mut self, keyboard: &mut ButtonInput<KeyCode>) -> bool {
        if !self.menu_open || !keyboard.just_pressed(KeyCode::Escape) {
            return false;
        }
        self.menu_open = false;
        keyboard.clear_just_pressed(KeyCode::Escape);
        true
    }

    pub(crate) fn close_menu(&mut self) {
        self.menu_open = false;
    }

    pub(crate) fn menu_open(&self) -> bool {
        self.menu_open
    }

    pub(crate) fn view(&self) -> ShadingView {
        ShadingView {
            options: Vec4::new(
                self.mode.index(),
                self.cavity as u8 as f32,
                self.normals as u8 as f32,
                self.background as u8 as f32,
            ),
            wire: Vec4::new(self.xray as u8 as f32, self.studio as u8 as f32, 0., 0.),
        }
    }
}
