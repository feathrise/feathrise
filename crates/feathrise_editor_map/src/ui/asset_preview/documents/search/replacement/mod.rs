//! 跨文件替换先中央差异预览、再原子改草稿；复用原保真变换、8份文档与各自撤销历史。

use bevy::prelude::*;
mod components;
mod constants;
mod control;
#[cfg(test)]
mod gpu_tests;
mod presentation;
mod resources;
mod restore;
mod scene;
mod tasks;
#[cfg(test)]
mod tests;
mod types;
mod worker;

#[cfg(test)]
pub(in super::super) use gpu_tests::gpu_assert_cross_replacement;
#[cfg(test)]
pub(in super::super) use gpu_tests::{gpu_prepare_save_batch, gpu_click_save_batch};
pub(crate) use resources::AssetReplaceState;

pub(super) fn install(app: &mut App) {
    app.init_resource::<AssetReplaceState>()
        .add_systems(
            Update,
            (control::input, control::scroll)
                .chain()
                .after(super::control::input)
                .after(super::super::interaction::actions)
                .before(super::super::navigation::NavigationSystemSet::Input)
                .before(crate::ui::systems::scroll_editor_side_panels)
                .in_set(crate::MapEditorSystemSet::Input),
        )
        .add_systems(
            Update,
            tasks::drive
                .after(super::tasks::drive)
                .in_set(crate::MapEditorSystemSet::Tasks),
        )
        .add_systems(
            Update,
            restore::select
                .after(super::super::native::bind)
                .before(super::super::navigation::NavigationSystemSet::Navigate)
                .in_set(crate::MapEditorSystemSet::Presentation),
        )
        .add_systems(
            Update,
            (presentation::setup, presentation::sync)
                .chain()
                .after(super::super::workspace::sync)
                .after(super::super::presentation::sync)
                .in_set(crate::MapEditorSystemSet::Presentation),
        );
}
