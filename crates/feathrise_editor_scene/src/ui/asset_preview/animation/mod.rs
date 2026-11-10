//! Blender式动作时间轴：真实GLB关键帧、骨骼预览、独立历史及受检原子保存。

use crate::SceneEditorSystemSet;
use bevy::prelude::*;

mod binding;
mod components;
mod context;
mod control;
mod data;
mod drive;
mod editing;
#[cfg(test)]
mod editor_tests;
mod pose;
mod presentation;
mod preview;
pub(crate) mod project_close;
mod resources;
pub(super) mod scene;
mod timeline;
mod types;
// 仅显式自有工程验收使用的原控件桥；实现留在有限夹具域，不公开作者文档/任务类型。
#[cfg(any(test, feature = "validation"))]
#[path = "../../../standalone/author_workflow_validation/animation_assets/animation_bridge.rs"]
pub(crate) mod workflow_validation;

pub(super) use preview::PreviewRuntime as AnimationPreviewRuntime;
pub(crate) use resources::AnimationEditorState;

/// 生产与集成回归共用输入注册，避免测试另写顺序掩盖场景批量域先消费按键的问题。
fn install_input(app: &mut App) {
    app.add_systems(Update, (control::input, timeline::input).chain()
            .after(super::workspace::actions)
            .after(super::interaction::handle_asset_preview_actions)
            .before(super::navigation::control_asset_preview)
            .before(crate::authoring::mesh_edit::MeshEditInput) // 首次按下即发布时间轴所有权，不能让场景建模先消费同帧快捷键。
            .before(crate::authoring::placement_batch::handle_placement_batch) // 右侧动画与场景可同屏，复制/粘贴/删除也必须先由悬停时间轴消费。
            .before(crate::ui::numeric_scrub_mouse_guard::update_scene_editor_numeric_scrub_mouse_guard)
            .in_set(SceneEditorSystemSet::Input));
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<AnimationEditorState>();
    install_input(app);
    app
        .add_systems(Update, drive::drive
            .after(super::loading::sync_asset_preview_selection)
            .before(super::loading::poll_asset_preview_loading)
            .in_set(SceneEditorSystemSet::Tasks))
        .add_systems(Update, (scene::setup_numeric_inputs, presentation::sync).chain()
            .after(super::presentation::sync_asset_preview_controls)
            .after(super::subasset_presentation::sync_asset_preview_subassets)
            .in_set(SceneEditorSystemSet::Presentation));
    preview::install(app);
    binding::install(app);
    pose::install(app);
}
