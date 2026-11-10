//! 唯一临时文件标签菜单；关闭点击和松键帧仍保留鼠标所有权。
use bevy::prelude::*;

#[derive(Resource, Default)]
pub(crate) struct AssetTabMenuState {
    pub(super) open: bool,
    pub(super) anchor: Vec2,
    pub(super) captured: bool,
    pub(super) scene_id: Option<feathrise_scene_content::SceneId>, // 菜单不能跨已完成的场景切换复用。
    pub(super) path: Option<String>, // 冻结右击标签；后台文件与当前活动文档可以不同。
}

impl AssetTabMenuState {
    pub(crate) fn owns_mouse(&self) -> bool {
        self.open || self.captured
    }
}
