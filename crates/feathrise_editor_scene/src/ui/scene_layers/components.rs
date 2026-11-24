//! 作者内容树的四个常驻列表、标题和固定行槽。

use super::types::{AuthorLayerCategory, LayerList};
use bevy::prelude::*;

#[derive(Component, Clone, Default)]
pub(super) struct Section(pub LayerList);
#[derive(Component, Clone, Default)]
pub(super) struct Title(pub LayerList);
#[derive(Component, Clone, Copy)]
pub(super) struct SectionSelection(pub AuthorLayerCategory);
#[derive(Component, Clone)]
pub(super) struct SectionArrow {
    pub list: LayerList,
    pub scene: Option<feathrise_scene_content::SceneId>,
}
#[derive(Component, Clone, Default)]
pub(super) struct List(pub LayerList);
#[derive(Component, Clone, Default)]
pub(super) struct Content;
#[derive(Component, Clone, Default)]
pub(super) struct RowLabel;
#[derive(Component, Clone, Default)]
pub(super) struct Row {
    pub list: LayerList,
    pub slot: usize,
}

#[derive(Component, Clone, Copy)]
pub(in crate::ui) struct VegetationRowToggle(pub crate::ui::scene_sidebar::TreeRowToggle);

/// 静水体行内控制的种类；来源和层 ID 由同一个行槽的绑定提供。
#[derive(Component, Clone, Copy)]
pub(crate) struct StillWaterRowToggle(pub crate::ui::scene_sidebar::TreeRowToggle);

/// 分类行内按钮按场景归属，和分支开合、名称选择各用自己的释放点击。
#[derive(Component, Clone)]
pub(super) struct SectionToggle {
    pub list: LayerList,
    pub toggle: crate::ui::scene_sidebar::TreeRowToggle,
    pub scene: Option<feathrise_scene_content::SceneId>,
}
