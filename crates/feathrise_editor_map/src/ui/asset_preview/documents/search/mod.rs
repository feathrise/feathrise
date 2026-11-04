//! assets内跨文件字面搜索；固定表单、虚拟结果和原文档中央定位共用既有输入边界。

use bevy::prelude::*;
mod components;
mod constants;
mod control;
#[cfg(test)]
mod gpu_tests;
mod jump;
mod missing_assets; // 只检查config/maps的明确路径，复用只读导航和唯一搜索槽。
mod presentation;
mod references; // JSON明确路径引用复用搜索槽；不进入跨文件替换。
mod resources;
pub(super) mod replacement; // 有界中央差异预览与原子草稿替换，不自动写盘。
mod scene;
mod tasks;
#[cfg(test)]
mod tests;
#[cfg(test)]
pub(super) use gpu_tests::gpu_assert_search;
#[cfg(test)]
pub(super) use references::gpu_assert_references;
#[cfg(test)]
pub(super) use missing_assets::gpu_assert_missing_assets;
mod types;
mod worker;

pub(crate) use resources::AssetSearchState;
pub(super) use control::{SearchGate, busy}; // 批量保存复用同一模态与未确认输入门。
pub(crate) use replacement::AssetReplaceState;
pub(crate) use scene::panel;

pub(super) fn install(app: &mut App) {
    crate::ui::external_files::register_requests(app);
    replacement::install(app);
    app.init_resource::<AssetSearchState>()
        .add_systems(Update, (control::input, control::scroll).chain()
            .after(crate::map_browser::handle_map_browser_input)
            .after(super::super::AssetTabMenuInputSet)
            .before(crate::ui::numeric_scrub_mouse_guard::update_map_editor_numeric_scrub_mouse_guard)
            .before(super::navigation::NavigationSystemSet::Input)
            .before(crate::ui::systems::scroll_editor_side_panels)
            .in_set(crate::MapEditorSystemSet::Input))
        .add_systems(Update, tasks::drive.after(super::session::drive_tasks).in_set(crate::MapEditorSystemSet::Tasks))
        .add_systems(Update, jump::prepare.before(super::native::bind).in_set(crate::MapEditorSystemSet::Presentation))
        .add_systems(Update, jump::select.after(super::navigation::NavigationSystemSet::Navigate).in_set(crate::MapEditorSystemSet::Presentation))
        .add_systems(Update, (presentation::setup, presentation::sync).chain().in_set(crate::MapEditorSystemSet::Presentation))
        .add_systems(PostUpdate, jump::reveal.after(super::navigation::NavigationSystemSet::Reveal)
            .before(bevy::ui_widgets::ImeSystems::UpdatePosition).in_set(bevy::ui::UiSystems::PostLayout));
}
