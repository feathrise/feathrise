//! 宿主诊断到原中央源码的受检导航；加载、草稿、历史及原生输入继续由唯一文档会话拥有。

mod resources;
mod systems;
mod types;
#[cfg(feature = "validation")]
pub(crate) mod validation;
#[cfg(test)]
mod tests;

use bevy::prelude::*;

pub(super) fn install(app: &mut App) {
    app.add_message::<crate::standalone::OpenProjectSourceLocation>()
        .init_resource::<resources::SourceLocationState>()
        .add_systems(Update, systems::receive
            .after(crate::ui::project_browser::ProjectBrowserTaskSet)
            .after(crate::ui::project_browser::ProjectBrowserRoutingSet)
            .before(super::session::sync_selection)
            .in_set(crate::SceneEditorSystemSet::Tasks))
        .add_systems(Update, systems::prepare
            .after(super::session::drive_tasks)
            .in_set(crate::SceneEditorSystemSet::Tasks))
        .add_systems(Update, systems::select
            .after(super::presentation::sync)
            .after(super::navigation::NavigationSystemSet::Navigate)
            .in_set(crate::SceneEditorSystemSet::Presentation))
        .add_systems(PostUpdate, systems::reveal
            .after(super::presentation::restore_source_scroll)
            .before(bevy::ui_widgets::ImeSystems::UpdatePosition)
            .in_set(bevy::ui::UiSystems::PostLayout));
}
