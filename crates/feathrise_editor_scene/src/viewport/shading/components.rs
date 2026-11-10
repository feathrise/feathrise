//! 着色控件和唯一作者相机的提取参数；不向世界Mesh挂临时材质。
use super::types::ShadingAction;
use bevy::prelude::*;
pub(super) use feathrise_scene::author_view::ShadingView;

#[derive(Component)]
pub(crate) struct ViewShadingRoot;
#[derive(Component)]
pub(super) struct ShadingMenu;
#[derive(Component)]
pub(crate) struct ShadingButton(pub(super) ShadingAction);
#[derive(Component)]
pub(super) struct ShadingOptionText(pub(super) ShadingAction);

#[cfg(test)]
pub(super) use feathrise_scene::author_view::PrepassLease;
