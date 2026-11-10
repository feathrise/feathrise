//! 资产浏览器可切换的静态缩略图：十二槽网格、串行GPU烘焙和有限缓存。
//! 【功能设计】列表身份、选择和大预览不变；只有当前可见缩略图按需生成。
//! 文件检查单飞后台执行，场景分帧隔离/退役，完整GPU回执与文件身份一致才采用。

use crate::SceneEditorSystemSet;
use bevy::{camera::visibility::VisibilitySystems, prelude::*};

mod components;
mod constants;
mod demand;
mod io;
mod jobs;
mod presentation;
mod render;
mod resources;
mod types;

pub(crate) use presentation::{spawn_thumbnail_nodes, view_mode_controls, grid_layout};
pub(crate) use constants::{TILE_HEIGHT, TILE_GAP};

pub(crate) fn install(app: &mut App) {
    super::asset_preview::compatible_texture::install(app); // 独立缩略图夹具与正式编辑器使用相同私有加载器。
    app.init_resource::<resources::ThumbnailState>()
        .add_systems(
            Update,
            demand::control_view_mode
                .after(
                    super::numeric_scrub_mouse_guard::update_scene_editor_numeric_scrub_mouse_guard,
                )
                .in_set(SceneEditorSystemSet::Input),
        )
        .add_systems(
            Update,
            (
                demand::sync_demand,
                jobs::poll_reloaded_assets,
                jobs::drive_jobs,
                jobs::retire_nodes,
            )
                .chain()
                .after(crate::asset_catalog::poll_asset_scan)
                .in_set(SceneEditorSystemSet::Tasks),
        )
        .add_systems(
            Update,
            presentation::sync_presentation
                .after(super::rows::sync_asset_rows)
                .after(super::rows::sync_dynamic_list_layouts)
                .in_set(SceneEditorSystemSet::Presentation),
        )
        .add_systems(
            PostUpdate,
            jobs::advance_scene
                .after(TransformSystems::Propagate)
                // 包围盒未发布时任务会等后续帧，保留本帧隐藏与退役早于可见性传播。
                .before(VisibilitySystems::VisibilityPropagate),
        )
        .add_observer(jobs::world_ready);
    render::install(app);
}

#[cfg(test)]
mod tests;

/// 布局夹具只安装原表现系统，不启动缩略图IO、GPU烘焙或增加相机。
#[cfg(test)]
pub(crate) fn install_presentation_test(app: &mut App) {
    app.init_resource::<resources::ThumbnailState>()
        .add_systems(
            Update,
            presentation::sync_presentation
                .after(super::rows::sync_asset_rows)
                .after(super::rows::sync_dynamic_list_layouts),
        );
}
