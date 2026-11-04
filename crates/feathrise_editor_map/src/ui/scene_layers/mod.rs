//! 场景树补齐水域、植被作者层、角色层与点位；固定行池直接读取当前作者文档。

mod components;
mod focus;
mod resources;
mod rows;
mod scene;
mod selection;
mod systems;
mod types;

pub(crate) use resources::SceneLayerSelection;
pub(crate) use scene::panel;
pub(crate) use selection::{LayerSelectionContext, select};

pub(crate) fn install(app: &mut bevy::prelude::App) {
    use bevy::prelude::*;
    app.init_resource::<SceneLayerSelection>()
        .init_resource::<resources::LayerFocusState>()
        .add_systems(Update, focus::update.after(crate::ui::scene_context_menu::SceneMenuDispatch)
            .in_set(crate::MapEditorSystemSet::Input))
        .add_systems(Update, (crate::ui::water_layers::handle_scene_menu, crate::ui::vegetation_layers::handle_scene_menu)
            .after(crate::ui::scene_context_menu::SceneMenuDispatch)
            .in_set(crate::MapEditorSystemSet::Input))
        .add_systems(Update, systems::input
            .after(crate::ui::numeric_scrub_mouse_guard::update_map_editor_numeric_scrub_mouse_guard)
            .before(crate::ui::scene_context_menu::SceneMenuDispatch)
            .in_set(crate::MapEditorSystemSet::Input))
        .add_systems(Update, systems::refresh_selection
            .after(crate::game_authoring::map_interaction::MapInteractionAdoption)
            .before(crate::viewport::poll_layer_candidates).in_set(crate::MapEditorSystemSet::Tasks))
        .add_systems(Update, systems::sync.in_set(crate::MapEditorSystemSet::Presentation));
}

#[cfg(test)]
pub(crate) mod tests;
