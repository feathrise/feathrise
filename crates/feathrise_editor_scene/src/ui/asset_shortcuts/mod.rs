//! 资产收藏与最近访问：固定行池、项目相对身份、单槽后台个人记录；不修改作者数据。

use crate::SceneEditorSystemSet;
use bevy::prelude::*;

mod components;
mod constants;
#[cfg(test)]
mod gpu_tests;
mod helpers;
mod messages;
mod resources;
mod scene;
mod systems;
mod tasks;
#[cfg(test)]
mod tests;
mod types;
mod worker;
#[cfg(test)]
pub(crate) use gpu_tests::assert_gpu;

#[cfg(test)]
pub(crate) fn install_for_tests(app: &mut App, root: std::path::PathBuf) {
    install(app);
    app.world_mut().resource_mut::<AssetShortcuts>().root = root;
}

pub(crate) use components::ShortcutList;
pub(crate) use messages::{AssetVisited, SetAssetFavorite};
pub(crate) use resources::AssetShortcuts;
pub(crate) use scene::{panel, toolbar_buttons};

pub(crate) fn register(app: &mut App) {
    app.init_resource::<AssetShortcuts>();
    if !app
        .world()
        .contains_resource::<Messages<SetAssetFavorite>>()
    {
        app.add_message::<SetAssetFavorite>();
    }
    if !app.world().contains_resource::<Messages<AssetVisited>>() {
        app.add_message::<AssetVisited>();
    }
}

pub(crate) fn install(app: &mut App) {
    register(app);
    app.add_systems(
        Update,
        systems::control
            .after(crate::asset_catalog::handle_asset_row_clicks)
            .after(super::numeric_scrub_mouse_guard::update_scene_editor_numeric_scrub_mouse_guard)
            .in_set(SceneEditorSystemSet::Input),
    )
    .add_systems(
        Update,
        (tasks::drive, scene::setup_rows).in_set(SceneEditorSystemSet::Tasks),
    )
    .add_systems(Update, scene::sync.in_set(SceneEditorSystemSet::Presentation));
}
