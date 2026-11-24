//! 场景树补齐水域、植被作者层、角色层与点位；固定行池直接读取当前作者文档。

mod components;
mod catalog;
mod focus;
mod inventory;
mod resources;
mod rows;
mod scene;
mod selection;
mod systems;
mod types;

pub(crate) use resources::{LayerCategoryVisibility, SceneLayerSelection, WaterLayerEyes};
pub(crate) use types::AuthorLayerCategory;
pub(in crate::ui) use components::VegetationRowToggle;
#[cfg(feature = "validation")]
pub(crate) use components::StillWaterRowToggle;
pub(crate) use catalog::VegetationCatalogAssets;
pub(in crate::ui) use inventory::vegetation_source;
pub(crate) use scene::panel;
pub(crate) use selection::{LayerSelectionContext, select};
pub(in crate::ui) use selection::request_cold_vegetation;
pub(in crate::ui) use systems::input as read_tree_selection;

pub(crate) fn install(app: &mut bevy::prelude::App) {
    use bevy::prelude::*;
    app.init_resource::<SceneLayerSelection>()
        .init_resource::<resources::LayerFocusState>()
        .init_resource::<resources::LayerTreeFolds>()
        .init_resource::<LayerCategoryVisibility>()
        .add_systems(Update, focus::update.after(crate::ui::scene_context_menu::SceneMenuDispatch)
            .in_set(crate::SceneEditorSystemSet::Input))
        .add_systems(Update, (crate::ui::water_layers::handle_scene_menu, crate::ui::vegetation_layers::handle_scene_menu)
            .after(crate::ui::scene_context_menu::SceneMenuDispatch)
            .in_set(crate::SceneEditorSystemSet::Input))
        .add_systems(Update, systems::input
            .after(crate::ui::numeric_scrub_mouse_guard::update_scene_editor_numeric_scrub_mouse_guard)
            .before(crate::ui::scene_context_menu::SceneMenuDispatch)
            .in_set(crate::SceneEditorSystemSet::Input))
        .add_systems(Update, (systems::fold_sections, systems::toggle_categories, systems::toggle_still_water)
            .after(crate::ui::numeric_scrub_mouse_guard::update_scene_editor_numeric_scrub_mouse_guard)
            .in_set(crate::SceneEditorSystemSet::Input))
        .add_systems(Update, systems::refresh_selection
            .after(crate::game_authoring::scene_interaction::SceneInteractionAdoption)
            .before(crate::viewport::poll_layer_candidates).in_set(crate::SceneEditorSystemSet::Tasks))
        .add_systems(Update, (systems::sync, systems::sync_vegetation_toggles, systems::sync_section_arrows, systems::sync_category_toggles,
            crate::authoring::layer_categories::sync_layer_category_preview,
            crate::authoring::layer_categories::sync_still_water_preview).chain()
            .in_set(crate::SceneEditorSystemSet::Presentation));
}

#[cfg(test)]
pub(crate) mod tests;
#[cfg(test)]
mod cold_vegetation_tests;
