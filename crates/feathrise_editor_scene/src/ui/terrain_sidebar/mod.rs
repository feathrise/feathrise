//! 地形侧栏顶部工具页及统一反馈；只重排UI，不拥有作者事务或持久化状态。

mod components;
mod resources;
mod scene;
mod systems;
mod volume_brush;

use bevy::prelude::*;

use crate::SceneEditorSystemSet;

pub(crate) use scene::terrain_tools_panel;
pub(crate) use volume_brush::spawn_types as spawn_volume_brush_types;

pub(crate) fn install(app: &mut App) {
    app.init_resource::<resources::TerrainSidebarState>()
        .add_systems(
            Update,
            volume_brush::handle
                .after(SceneEditorSystemSet::Input)
                .after(feathrise_editor_ui::EditorUiSystemSet::NumericInput)
                .before(SceneEditorSystemSet::Tasks),
        )
        .add_systems(
            Update,
            systems::handle_tabs
                .after(SceneEditorSystemSet::Input)
                .before(crate::viewport::modes::sync_mode)
                .before(SceneEditorSystemSet::Tasks),
        )
        .add_systems(
            Update,
            (
                volume_brush::setup,
                volume_brush::sync,
                systems::sync_pages.after(crate::ui::actions::sync_editor_action_button_styles),
                systems::sync_tools,
                systems::sync_feedback
                    .after(crate::authoring::apply_sparse_terrain_brush)
                    .after(crate::authoring::apply_sparse_terrain_brush)
                    .after(crate::authoring::apply_surface_brush)
                    .after(crate::authoring::apply_puddle_brush),
            )
                .chain()
                .in_set(SceneEditorSystemSet::Presentation),
        );
}

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) fn install_snapshot_presentation(app: &mut App) {
    app.init_resource::<resources::TerrainSidebarState>().add_systems(Update, systems::sync_pages);
}
