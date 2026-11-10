//! 场景目录的唯一右键操作浮层；只冻结并派发原按钮，不另建场景或文件夹事务。

mod components;
mod resources;
mod scene;
mod systems;
mod types;
#[cfg(test)]
mod snapshot;

#[cfg(test)]
pub(crate) use snapshot::{DirectoryMenuSnapshot, install_snapshot, assert_snapshot_bounds};

pub(crate) use resources::SceneDirectoryMenuState;

use bevy::prelude::*;

pub(super) fn install(app: &mut App) {
    app.init_resource::<SceneDirectoryMenuState>()
        .add_systems(
            Update,
            systems::control_menu
                .after(crate::scene_browser::handle_scene_folder_dialog_input)
                .before(crate::scene_browser::handle_scene_browser_input)
                .before(crate::ui::asset_context_menu::AssetMenuInput)
                .in_set(crate::SceneEditorSystemSet::Input),
        )
        .add_systems(
            Update,
            scene::setup.in_set(crate::SceneEditorSystemSet::Tasks),
        )
        .add_systems(
            Update,
            scene::sync
                .after(super::systems::sync_sidebar)
                .in_set(crate::SceneEditorSystemSet::Presentation),
        );
}

#[cfg(test)]
mod tests;
