//! 吸附工具条动作与作者相机的无限参考网格参数，不复制持久偏好。
use bevy::prelude::*;
pub(crate) use feathrise_scene::author_view::GridView;


#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SnapAction {
    #[default]
    Grid,
    Enabled,
    Mode,
    Step,
}

#[derive(Component, Clone, Default)]
pub(crate) struct SnapButton(pub SnapAction);

#[derive(Component, Clone, Default)]
pub(crate) struct SnapLabel(pub SnapAction);
