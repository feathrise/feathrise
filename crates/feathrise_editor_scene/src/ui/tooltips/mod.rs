//! 顶栏与视口按钮的单一只读悬停提示；键位来自实际偏好，浮层不改变布局或作者动作。

mod components;
mod helpers;
mod resources;
mod scene;
mod systems;
mod types;

pub(crate) use components::EditorTooltipTarget;
pub(crate) use components::TooltipPopup;
pub(super) use components::TooltipHeader;
pub(super) use scene::tooltip_popup;
pub(crate) use types::{TooltipAction, TooltipPositioned};

use bevy::prelude::*;

pub(crate) fn install(app: &mut App) {
    app.init_resource::<resources::EditorTooltipState>()
        .add_systems(
            PostUpdate,
            (systems::setup_header_targets, systems::sync_tooltip)
                .chain()
                .before(bevy::ui::UiSystems::Prepare)
                .before(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate),
        )
        .add_systems(
            PostUpdate,
            systems::position_tooltip
                .in_set(TooltipPositioned)
                .after(bevy::ui::UiSystems::PostLayout)
                .after(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate),
        );
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod gpu_tests;
