//! 独立编辑器的已保存试玩合同；只改变F12/启动入口，不复制原作者工具或隐式保存。

#[cfg(any(test, feature = "validation"))]
mod author_workflow_validation;
mod cursor; // 独立作者拥有系统光标写侧，不再依赖原游戏GameModePlugin。
mod embedded_input; // 仅模式交接短暂隔离输入，稳定原生预览仍允许操作作者工作台。
mod embedded_presentation; // 只发布真实预览模式，原工作台保持可见，返回不重开作者会话。
pub mod embedded_placement_preview; // 原对象手柄与数值手势的有界实时投影，独立于整图保存/快照。
pub mod embedded_snapshot; // 可选常驻工程视口只读捕获，默认不开启，不改变保存后试玩合同。
pub mod embedded_vegetation; // 原连续笔刷已发布的正式草密度独立投影，不等待松手或整场景捕获。
pub mod embedded_author_preview; // 原网格/高度/水工具已采用的局部结果，不重建第二套编辑业务。
pub mod embedded_surface_trail; // 原地表即时辅助图的有界行块投影，PBR仍在原松笔流程采用。
pub mod embedded_overlay; // 原作者工具线条的有界只读投影，供宿主桥接到游戏视口。
pub mod embedded_tools; // 笔刷、变换手柄和三维选框的语义投影。
pub mod viewport_projection; // 仅公开窗口几何与原输入门结论，不公开作者内部资源。
mod exit;
mod messages;
mod navigation;
pub(crate) mod project_close;
mod resources;
mod saved_play;
#[cfg(feature = "validation")]
pub mod scene_format_validation;
mod systems;
#[cfg(test)]
mod tests;
#[cfg(any(test, feature = "validation"))]
pub mod validation_fixture;
#[cfg(any(test, feature = "validation"))]
pub mod embedded_validation;
#[cfg(feature = "validation")]
pub use crate::ui::asset_preview::rigging_validation::install as install_rigging_validation;
#[cfg(feature = "validation")]
pub use crate::ui::asset_preview::project_browser_validation::install as install_project_browser_validation;
#[cfg(feature = "validation")]
pub use crate::ui::file_manager_validation::install as install_file_manager_validation;

pub use exit::exit_blocker;
pub use embedded_presentation::set_embedded_playing;
pub use messages::{OpenProjectSourceLocation, RunSavedProject, SavedPlayFeedback};
pub use navigation::{author_status, preview_ready_for, request_open_scene};
#[cfg(feature = "validation")]
pub use crate::ui::asset_preview::documents::source_location_validation::{
    SourceLocationAction, source_location_action_entity, source_location_input_entity, source_location_navigation_probe, source_location_probe,
};
pub use project_close::{
    ProjectAuthorAdapter, ProjectAuthorAdapters, ProjectAuthorDocument, ProjectAuthorSnapshot,
    ProjectCloseSaveAdapters, ProjectCloseSnapshot, ProjectSavePlugin, ProjectSaveProgress, cancel_project_save,
    embedded_play_blocker, project_close_snapshot, project_save_progress, request_save_all,
};
pub use resources::{EmbeddedAuthorInput, StandaloneEditorMode};
pub(crate) use resources::StandalonePlayRequest;
pub use saved_play::{current_saved_play_request, saved_play_blocker};

use bevy::prelude::*;

/// 仅独立编辑App安装；原一体入口不安装时仍使用原F12安全关闭与同World玩法。
pub struct StandaloneEditorPlugin;

/// 原作者任务与本帧F12意图已完成发布，宿主随后才能做跨作者域的保存预检。
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SavedPlayDispatch;

impl Plugin for StandaloneEditorPlugin {
    fn build(&self, app: &mut App) {
        project_close::install(app);
        cursor::install(app);
        embedded_input::install(app);
        app.init_resource::<StandaloneEditorMode>()
            .init_resource::<feathrise_editor_core::ProjectLaunchSettings>()
            .add_message::<RunSavedProject>()
            .add_message::<SavedPlayFeedback>()
            .add_systems(
                Update,
                (systems::publish_saved_play, systems::receive_feedback)
                    .chain()
                    .in_set(SavedPlayDispatch)
                    .after(feathrise_editor_core::SceneEditorSystemSet::Tasks),
            );
    }
}
