//! 普通点击只发布当前帧的稳定对象身份，由菜单共用的唯一选择出口收口。

use bevy::prelude::*;

#[derive(Resource, Default)]
pub(crate) struct SceneLayerSelection(pub Option<crate::ui::scene_context_menu::SceneObjectTarget>);

/// 分类折叠属于场景树界面会话，画布和作者修订换代不会重置。
#[derive(Resource, Default)]
pub(super) struct LayerTreeFolds {
    scene: Option<feathrise_scene_content::SceneId>,
    closed: [bool; 4],
}

impl LayerTreeFolds {
    pub fn collapsed(&self, scene: &feathrise_scene_content::SceneId, list: super::types::LayerList) -> bool {
        self.scene.as_ref() == Some(scene) && self.closed[list as usize]
    }

    pub fn toggle(&mut self, scene: &feathrise_scene_content::SceneId, list: super::types::LayerList) {
        if self.scene.as_ref() != Some(scene) {
            self.scene = Some(scene.clone());
            self.closed = [false; 4];
        }
        self.closed[list as usize] ^= true;
    }
}

#[derive(Resource, Default)]
pub(super) struct LayerFocusState {
    pub pending: Option<super::types::PendingFocus>,
    pub desired: Option<crate::ui::scene_context_menu::SceneObjectTarget>,
}
