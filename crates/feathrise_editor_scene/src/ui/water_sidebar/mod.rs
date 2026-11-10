//! 水域侧栏的横向工具页与只读反馈；统一列表、数值输入和作者事务仍由原领域维护。

mod components;
mod resources;
mod systems;
mod types;

use crate::SceneEditorSystemSet;
use bevy::prelude::*;

pub(crate) use components::*;
pub(crate) use resources::WaterSidebarPage;
pub(crate) use types::WaterSidebarFeedbackKind;

pub(crate) fn install(app: &mut App) {
    app.init_resource::<resources::WaterSidebarState>()
        .add_systems(
            Update,
            systems::handle_tabs
                .after(SceneEditorSystemSet::Input)
                .before(crate::viewport::modes::sync_mode)
                .before(SceneEditorSystemSet::Tasks),
        )
        .add_systems(
            Update,
            (systems::sync_pages, systems::sync_feedback)
                .chain()
                .after(crate::authoring::apply_water_brush)
                .in_set(SceneEditorSystemSet::Presentation),
        );
}

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) fn install_snapshot_presentation(app: &mut App) {
    app.init_resource::<resources::WaterSidebarState>().add_systems(Update, systems::sync_pages);
}
