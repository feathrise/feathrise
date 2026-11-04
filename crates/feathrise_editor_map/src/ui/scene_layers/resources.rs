//! 普通点击只发布当前帧的稳定对象身份，由菜单共用的唯一选择出口收口。

use bevy::prelude::*;

#[derive(Resource, Default)]
pub(crate) struct SceneLayerSelection(pub Option<crate::ui::scene_context_menu::SceneObjectTarget>);

#[derive(Resource, Default)]
pub(super) struct LayerFocusState {
    pub pending: Option<super::types::PendingFocus>,
    pub desired: Option<crate::ui::scene_context_menu::SceneObjectTarget>,
}
