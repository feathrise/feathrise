//! JSON、Rust和WGSL源码着色；后台分词，只复用现有排版，不接管原生输入。

use bevy::{
    prelude::*,
    render::{ExtractSchedule, RenderApp},
    ui_render::RenderUiSystems,
};

mod components;
mod constants;
mod extract;
mod index;
mod lexer;
mod paint;
mod resources;
#[cfg(test)]
mod tests;
mod types;

pub(super) use components::SyntaxPreview;
#[cfg(test)]
pub(super) use tests::{gpu_assert_input, gpu_assert_syntax};

pub(super) fn install(app: &mut App) {
    app.init_resource::<resources::SyntaxIndex>()
        .init_resource::<feathrise_editor_ui::EditorPalette>() // 独立文档GPU测试和完整F12共用同一色板。
        .init_resource::<resources::SyntaxPaint>()
        .add_systems(
            Update,
            index::drive
                .after(super::session::drive_tasks)
                .in_set(crate::MapEditorSystemSet::Tasks),
        )
        .add_systems(
            PostUpdate,
            paint::sync.after(bevy::ui::UiSystems::PostLayout),
        );
    if let Some(render) = app.get_sub_app_mut(RenderApp) {
        render
            .init_resource::<resources::SyntaxExtraction>()
            .add_systems(
                ExtractSchedule,
                extract::invalidate
                    .after(RenderUiSystems::ExtractChanges)
                    .before(RenderUiSystems::ExtractCameraViews),
            )
            .add_systems(
                ExtractSchedule,
                extract::begin
                    .after(RenderUiSystems::ExtractTextShadows)
                    .before(RenderUiSystems::ExtractText),
            )
            .add_systems(
                ExtractSchedule,
                extract::color
                    .after(RenderUiSystems::ExtractText)
                    .before(RenderUiSystems::ExtractCursor),
            );
    }
}
