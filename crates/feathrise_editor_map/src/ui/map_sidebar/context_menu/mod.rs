//! 地图目录的唯一右键操作浮层；只冻结并派发原按钮，不另建地图或文件夹事务。

mod components;
mod resources;
mod scene;
mod systems;
mod types;
#[cfg(test)]
mod snapshot;

#[cfg(test)]
pub(crate) use snapshot::{DirectoryMenuSnapshot, install_snapshot, assert_snapshot_bounds};

pub(crate) use resources::MapDirectoryMenuState;

use bevy::prelude::*;

pub(super) fn install(app: &mut App) {
    app.init_resource::<MapDirectoryMenuState>()
        .add_systems(
            Update,
            systems::control_menu
                .after(crate::map_browser::handle_map_folder_dialog_input)
                .before(crate::map_browser::handle_map_browser_input)
                .before(crate::ui::asset_context_menu::AssetMenuInput)
                .in_set(crate::MapEditorSystemSet::Input),
        )
        .add_systems(
            Update,
            scene::setup.in_set(crate::MapEditorSystemSet::Tasks),
        )
        .add_systems(
            Update,
            scene::sync
                .after(super::systems::sync_sidebar)
                .in_set(crate::MapEditorSystemSet::Presentation),
        );
}

#[cfg(test)]
mod tests;
