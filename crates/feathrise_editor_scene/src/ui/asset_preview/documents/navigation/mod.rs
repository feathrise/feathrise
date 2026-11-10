//! 文档查找替换、逻辑行号与跳行；复用唯一原生输入，后台单槽、标记只处理可见行。

use bevy::prelude::*;

pub(super) mod components;
mod constants;
mod control;
mod index;
mod paint;
mod presentation;
mod replace;
#[cfg(test)]
mod replacement_tests;
#[cfg(test)]
mod replacement_gpu_tests;
mod resources;
mod reveal;
#[cfg(test)]
mod tests;
mod types;

#[cfg(test)]
pub(super) use tests::gpu_assert_navigation;
#[cfg(test)]
pub(super) use replacement_gpu_tests::gpu_assert_replacement;
pub(super) use types::NavigationSystemSet;
pub(super) use reveal::reveal_selection;
pub(super) use index::literal_regex;
pub(super) use replace::prepare_ranges; // 跨文件预览与应用共用原字节保真变换，不维护第二套换行算法。
pub(super) use types::ReplacedContent;

pub(super) fn install(app: &mut App) {
    app.init_resource::<resources::DocumentNavigation>()
        .init_resource::<resources::NavigationPaint>()
        .add_systems(
            Update,
            control::input
                .in_set(NavigationSystemSet::Input)
                .after(super::history::HistorySystemSet::Input)
                .after(super::interaction::actions)
                .before(crate::ui::actions::handle_editor_shortcuts)
                .in_set(crate::SceneEditorSystemSet::Input),
        )
        .add_systems(
            Update,
            index::drive
                .after(super::session::drive_tasks)
                .in_set(crate::SceneEditorSystemSet::Tasks),
        )
        .add_systems(
            Update,
            (presentation::sync, control::navigate, replace::apply)
                .chain()
                .in_set(NavigationSystemSet::Navigate)
                .after(super::presentation::sync)
                .in_set(crate::SceneEditorSystemSet::Presentation),
        )
        .add_systems(
            PostUpdate,
            (
                reveal::sync.in_set(NavigationSystemSet::Reveal),
                paint::sync,
            )
                .chain()
                .after(super::presentation::restore_source_scroll)
                .before(bevy::ui_widgets::ImeSystems::UpdatePosition)
                .in_set(bevy::ui::UiSystems::PostLayout),
        );
}
