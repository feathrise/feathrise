//! F12 全局静水外观预设：连续目录、中央多标签表单、引用保护与热采用。

mod assets;
mod components;
mod defaults;
mod fields;
mod form;
mod ids;
mod interaction;
mod live_preview; // 编辑所选使用右侧独立10m×50m样品河面，不接管当前地图。
mod persistence;
pub(crate) mod project_close;
mod presentation;
mod scene;
mod types;
mod workspace;
mod widgets;

#[cfg(test)]
mod workbench_tests;

use bevy::prelude::*;
use feathrise_atmosphere::WeatherSystemSet; // 样品相机在正式天气和世界光照得到最终值后再镜像环境。

pub(crate) use live_preview::{WaterPresetLivePreviewNavigation, water_live_preview_host}; // 右侧场景页挂载独立样品预览壳，统一输入门读取其导航所有权。
pub(crate) use scene::{water_preset_feedback, water_preset_panel};
pub(crate) use types::WaterPresetEditorState;
pub(crate) use workspace::{spawn_active_water_editor, spawn_water_tabs};

use crate::MapEditorSystemSet;

pub(crate) fn install(app: &mut App) {
    app.init_resource::<WaterPresetEditorState>()
        .init_resource::<live_preview::WaterPresetLivePreview>() // 活动编辑标签只发布固定样品外观。
        .init_resource::<live_preview::WaterPresetLivePreviewRuntime>() // 独立RTT、相机与材质保持稳定身份。
        .init_resource::<WaterPresetLivePreviewNavigation>() // 独立保存右键轨道、中键平移与滚轮距离。
        .add_systems(
            Update,
            live_preview::control_live_preview_camera
                .before(crate::ui::numeric_scrub_mouse_guard::update_map_editor_numeric_scrub_mouse_guard)
                .in_set(MapEditorSystemSet::Input), // 先发布样品手势所有权，统一鼠标门同帧阻止地图输入。
        )
        .add_systems(
            Update,
            (interaction::handle_text_input, interaction::handle_buttons)
                .chain()
                .in_set(MapEditorSystemSet::Input),
        )
        .add_systems(
            Update,
            (
                persistence::advance_delete_preflight, // 元数据检查有界推进，完成后才进入原后台写入事务。
                persistence::poll_catalog_write, // 先接收目录热采用，再投影当前完整样品快照。
                live_preview::sync_live_preview_draft,
                live_preview::setup_live_preview_scene, // UI壳首次出现时一次创建固定离屏场景。
                live_preview::sync_live_preview_camera, // 创建后立即应用导航状态与真实画布宽高比。
            )
                .chain()
                .in_set(MapEditorSystemSet::Tasks),
        )
        .add_systems(
            Update,
            (
                live_preview::sync_live_preview_environment_request, // 天空与IBL继续进入正式异步加载/热换链。
                live_preview::sync_live_preview_material, // 只替换固定样品材质并推进可见画面时间。
                live_preview::sync_live_preview_visibility, // 进入标签只揭示一次右侧场景页并启停专用相机。
                presentation::sync_panel,
                presentation::sync_document_badge,
                presentation::sync_button_styles,
            )
                .chain()
                .after(crate::ui::vegetation_preset_editor::PresetWorkspaceRebuildSet) // 共享正文提交后再同步页头和按钮，避免丢失换代边沿。
                .in_set(MapEditorSystemSet::Presentation),
        )
        .add_systems(
            Update,
            live_preview::sync_live_preview_lighting.after(WeatherSystemSet::ApplyEffects), // 复用本帧最终环境光、雾与太阳/月亮，不复制假灯。
        );
}
