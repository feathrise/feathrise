//! F4 动态场景数据监控功能域。
//!
//! 【功能设计】F4复刻F3的宽面板、两级分页、六类植被LOD控件和互斥正文；
//! 默认进入全图总览，并以低频缓存报告覆盖地形、地表、植被、水域、场景和玩法数据。

mod helpers;
mod lod_controls;
mod vegetation_target; // 自定义工程按当前作者层诊断，分类绑定只由消费应用提供。
mod report;
mod systems;
mod ui;

use bevy::prelude::{App, IntoScheduleConfigs, Startup, Update, PostUpdate, TransformSystems};
use feathrise_fps_display::LodControlState; // F3与F4必须读写本次进程内唯一逐档状态。

use crate::SceneEditorSystemSet;
use crate::authoring::start_vegetation_preview_rebuild;
use crate::monitor_resources::EditorSceneMonitorState;

/// 向 F12 开发工具插件安装独立 F4 编辑器场景监控。
pub(crate) fn install_editor_scene_monitor(app: &mut App) {
    app.init_resource::<EditorSceneMonitorState>()
        .init_resource::<LodControlState>()
        .add_systems(Startup, ui::spawn_editor_scene_monitor_panel)
        .add_systems(
            Update,
            (
                systems::toggle_editor_scene_monitor,
                systems::select_editor_scene_monitor_page,
                lod_controls::handle_vegetation_lod_control_interactions,
            )
                .chain()
                .in_set(SceneEditorSystemSet::Input),
        )
        .add_systems(
            Update,
            (
                lod_controls::update_vegetation_lod_control_ui,
                systems::refresh_editor_scene_monitor,
            )
                .chain()
                .after(start_vegetation_preview_rebuild)
                .in_set(SceneEditorSystemSet::Presentation),
        );
    install_lod_application(app);
}

/// 应用顺序是共享诊断归还旧身份，再由作者会话采用当前目标，最后传播本帧可见性。
fn install_lod_application(app: &mut App) {
    app.add_systems(
        PostUpdate,
        (lod_controls::apply_changed_vegetation_lods, lod_controls::apply_vegetation_lods_to_new_nodes)
            .chain()
            .after(feathrise_fps_display::LodControlApplySet)
            .before(TransformSystems::Propagate)
            .before(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate),
    );
}
