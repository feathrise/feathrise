//! 作者内容树的四个常驻列表、标题和固定行槽。

use super::types::LayerList;
use bevy::prelude::*;

#[derive(Component, Clone, Default)]
pub(super) struct Section(pub LayerList);
#[derive(Component, Clone, Default)]
pub(super) struct Title(pub LayerList);
#[derive(Component, Clone, Default)]
pub(super) struct List(pub LayerList);
#[derive(Component, Clone, Default)]
pub(super) struct Content;
#[derive(Component, Clone, Default)]
pub(super) struct Row {
    pub list: LayerList,
    pub slot: usize,
}
