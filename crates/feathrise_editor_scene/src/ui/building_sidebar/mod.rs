//! 建筑侧栏的顶部工具页与统一反馈区；不拥有预设、场景或作者事务。

mod components;
mod resources;
mod scene;
mod systems;

use bevy::prelude::*;

use crate::SceneEditorSystemSet;

pub(crate) use scene::building_tools_panel;

pub(crate) fn install(app: &mut App) {
    app.init_resource::<resources::BuildingSidebarPage>()
        .add_systems(
            Update,
            systems::handle_tabs
                .after(SceneEditorSystemSet::Input)
                .before(crate::viewport::modes::sync_mode)
                .before(SceneEditorSystemSet::Tasks),
        )
        .add_systems(
            Update,
            systems::sync_pages.in_set(SceneEditorSystemSet::Presentation),
        );
}

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) fn install_snapshot_presentation(app: &mut App) {
    app.init_resource::<resources::BuildingSidebarPage>()
        .init_resource::<crate::ui::courtyard::CourtyardUiState>()
        .add_systems(Update, systems::sync_pages);
}
