//! 顶栏及视口按钮的只读提示身份，与固定浮层节点分离。

use bevy::prelude::*;

use super::types::TooltipAction;

#[derive(Component, Clone, Copy, Default)]
pub(crate) struct EditorTooltipTarget(pub(crate) TooltipAction);

#[derive(Component, Clone, Copy, Default)]
pub(in crate::ui) struct TooltipHeader;

#[derive(Component, Clone, Copy, Default)]
pub(super) struct TooltipElement;

#[derive(Component, Clone, Copy, Default)]
#[require(TooltipElement)]
pub(crate) struct TooltipPopup; // 原生 child 遮挡投影只读此浮层显隐，不获取提示会话写权限。

#[derive(Component, Clone, Copy, Default)]
#[require(TooltipElement)]
pub(super) enum TooltipText {
    #[default]
    Title,
    Description,
    Reason,
}
