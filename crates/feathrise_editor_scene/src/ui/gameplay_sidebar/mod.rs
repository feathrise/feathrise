//! 玩法侧栏的固定工具页和只读反馈；摆放、传送和空气墙继续使用原作者事务。

mod components;
mod resources;
mod scene;
mod systems;
mod types;

use crate::SceneEditorSystemSet;
use bevy::prelude::*;

pub(crate) use resources::GameplaySidebarState;
pub(crate) use scene::gameplay_tools_panel;
pub(crate) use types::GameplaySidebarPage;

pub(crate) fn install(app: &mut App) {
    app.init_resource::<GameplaySidebarState>()
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
                .before(crate::ui::portal_list::spawn_portal_list_rows)
                .before(crate::ui::portal_list::sync_portal_selection_panel)
                .before(crate::ui::portal_list::sync_current_portal_rows)
                .before(crate::ui::portal_list::sync_target_portal_rows)
                .in_set(SceneEditorSystemSet::Presentation),
        );
}

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) fn install_snapshot_presentation(app: &mut App) {
    app.init_resource::<resources::GameplaySidebarState>().add_systems(Update, systems::sync_pages);
}
