//! 帮助只记本次查看来源及鼠标归属，不写设置、地图或撤销历史。

use super::types::{HelpPage, HelpTarget};
use bevy::prelude::*;

#[derive(Resource, Default)]
pub(crate) struct EditorContextHelpState {
    pub(super) active: Option<HelpTarget>,
    pub(super) page: Option<HelpPage>,
    pub(super) hovered: bool,
    pub(super) captured_this_frame: bool,
    pub(super) generation: u64,
    pub(super) from_settings: bool,
    /// 记录打开时的物理鼠标位置；阅读和滚动期间不追随鼠标，避免浮层逃离指针。
    pub(super) pointer: Option<Vec2>,
    pub(super) numeric_mouse: crate::resources::MapEditorNumericScrubMouseGuard,
}

impl EditorContextHelpState {
    pub(crate) fn overlays_viewport(&self) -> bool { self.active.is_some() }
    pub(crate) fn owns_mouse(&self) -> bool {
        (self.active.is_some() && self.hovered) || self.captured_this_frame
    }

    pub(crate) fn blocks_modal_input(&self) -> bool {
        (self.active.is_some() && self.from_settings) || self.captured_this_frame
    }
}
