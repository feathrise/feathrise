//! 场景视口的同排视图切换与上下文参数；原动作和数值输入桥仍是唯一业务入口。

mod components;
mod scene;
mod wrapping;

pub(super) use components::{ViewportBrushParameters, ViewportToolbarScroll};
#[cfg(feature = "validation")]
pub(super) use components::ViewportToolbarExpandButton;
pub(super) use scene::{
    control_group, control_surface, control_text, toolbar_layout, viewport_toolbar,
};
pub(crate) use wrapping::install;

#[cfg(test)]
mod tests;
