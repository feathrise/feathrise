//! F12 全局 PBR 地表预设：无分页选择、中央多标签编辑、数组派生与引用保护删除。

#[cfg(test)]
mod components;
mod systems;
mod selection;
mod interaction;
mod persistence;
mod presentation;
mod scene;
mod types;
mod workspace;
#[cfg(test)]
mod workbench_tests;

use bevy::prelude::*;

pub(crate) use scene::{surface_preset_feedback, surface_preset_panel};
pub(crate) use types::SurfacePresetEditorState;
pub(crate) use workspace::{spawn_active_surface_editor, spawn_surface_tabs};

use crate::SceneEditorSystemSet;

pub(crate) fn install(app: &mut App) {
    app.init_resource::<SurfacePresetEditorState>()
        .add_systems(
            Update,
            (interaction::handle_text_input, interaction::handle_buttons)
                .chain()
                .in_set(SceneEditorSystemSet::Input),
        )
        .add_systems(
            Update,
            persistence::poll_catalog_write.in_set(SceneEditorSystemSet::Tasks),
        )
        .add_systems(
            Update,
            (
                presentation::sync_panel,
                presentation::sync_button_styles,
                presentation::sync_document_badge,
            )
                .chain()
                .after(crate::ui::vegetation_preset_editor::PresetWorkspaceRebuildSet) // 正文换代提交后再同步新节点的状态与选中样式。
                .in_set(SceneEditorSystemSet::Presentation),
        );
}
