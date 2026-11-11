//! 着色控件和唯一作者相机的提取参数；不向世界Mesh挂临时材质。
use super::types::ShadingAction;
use bevy::prelude::*;
pub(super) use feathrise_scene::author_view::ShadingView;

#[derive(Component)]
pub(crate) struct ViewShadingRoot;

#[derive(Component)]
pub(crate) struct ShadingToolbarRoot;

/// 同一套按钮分别服务地图和中央资产预览，点击与样式均按宿主取状态。
#[derive(Component, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum ShadingHost {
    #[default]
    Map,
    Asset,
}

#[derive(Component)]
pub(super) struct ShadingMenu;
#[derive(Component)]
pub(crate) struct ShadingButton(pub(super) ShadingAction);
#[derive(Component)]
pub(super) struct ShadingOptionText(pub(super) ShadingAction);

#[cfg(test)]
pub(super) use feathrise_scene::author_view::PrepassLease;
