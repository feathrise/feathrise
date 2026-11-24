//! 普通点击只发布当前帧的稳定对象身份，由菜单共用的唯一选择出口收口。

use bevy::prelude::*;
use feathrise_scene_content::{SceneId, TerrainId};
use std::{collections::{BTreeMap, BTreeSet, HashMap}, sync::Arc};

#[derive(Resource, Default)]
pub(crate) struct SceneLayerSelection(pub Option<crate::ui::scene_context_menu::SceneObjectTarget>);

/// 分类眼睛只影响当前编辑器会话，与场景清单和成员显隐独立。
pub(crate) type WaterLayerEyes = Arc<BTreeMap<Option<TerrainId>, BTreeSet<String>>>;

#[derive(Resource, Default)]
pub(crate) struct LayerCategoryVisibility {
    categories: HashMap<SceneId, [bool; 2]>,
    water: HashMap<SceneId, WaterLayerEyes>,
    water_revision: u64,
}

impl LayerCategoryVisibility {
    pub(crate) fn hidden(&self, scene: &SceneId, vegetation: bool) -> bool {
        self.categories.get(scene).is_some_and(|flags| flags[usize::from(vegetation)])
    }

    pub(super) fn toggle(&mut self, scene: &SceneId, vegetation: bool) {
        let flags = self.categories.entry(scene.clone()).or_default();
        flags[usize::from(vegetation)] ^= true;
        if *flags == [false; 2] { self.categories.remove(scene); }
    }

    pub(crate) fn water_hidden(&self, scene: &SceneId, terrain: Option<&TerrainId>, id: &str) -> bool {
        self.water.get(scene).and_then(|sources| sources.get(&terrain.cloned())).is_some_and(|ids| ids.contains(id))
    }

    pub(crate) fn water_eyes(&self, scene: &SceneId) -> Option<&WaterLayerEyes> { self.water.get(scene) }
    pub(crate) fn water_revision(&self) -> u64 { self.water_revision }

    /// 图层眼睛按完整来源绑定，删除后释放 ID 对应的本地眼睛状态。
    pub(crate) fn set_water_hidden(&mut self, scene: &SceneId, terrain: Option<&TerrainId>, id: &str, hidden: bool) {
        if self.water_hidden(scene, terrain, id) == hidden { return; }
        let sources = Arc::make_mut(self.water.entry(scene.clone()).or_default());
        let terrain = terrain.cloned();
        if hidden {
            sources.entry(terrain).or_default().insert(id.to_owned());
        } else if let Some(ids) = sources.get_mut(&terrain) {
            ids.remove(id);
            if ids.is_empty() { sources.remove(&terrain); }
        }
        if sources.is_empty() { self.water.remove(scene); }
        self.water_revision = self.water_revision.wrapping_add(1);
    }
}

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
