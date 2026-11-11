//! 自绘顶栏的固定尺寸、窗口意图与物理命中合同，不引用场景或文档规模。
use bevy::prelude::*;

pub(super) const HEIGHT: f32 = 36.;
pub(super) const CAPTION_WIDTH: f32 = 44.;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Menu {
    More,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WindowAction {
    Minimize,
    Maximize,
    Close,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Glyph {
    Save,
    Undo,
    Redo,
    Filter,
    Settings,
    Navigation,
    Inspector,
    Focus,
    More,
    Minimize,
    Maximize,
    Restore,
    Close,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct PixelRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}
impl PixelRect {
    pub fn contains(self, x: i32, y: i32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }
    pub fn from_node(node: &ComputedNode, transform: &UiGlobalTransform) -> Self {
        let center = transform.translation;
        let half = node.size() / 2.;
        Self {
            left: (center.x - half.x).floor() as i32,
            top: (center.y - half.y).floor() as i32,
            right: (center.x + half.x).ceil() as i32,
            bottom: (center.y + half.y).ceil() as i32,
        }
    }
}

#[derive(Resource, Default, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ChromeGeometry {
    pub drag: PixelRect,
    pub minimize: PixelRect,
    pub maximize: PixelRect,
    pub close: PixelRect,
    pub visible: bool,
}
