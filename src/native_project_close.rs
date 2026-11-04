//! 工程级关闭事务：聚合原作者保存回执，得到明确授权后才退役本工程实例。

#[path = "native_project_close_components.rs"]
mod components;
#[path = "native_project_close_resources.rs"]
mod resources;
#[path = "native_project_close_systems.rs"]
mod systems;
#[cfg(test)]
#[path = "native_project_close_tests.rs"]
pub(crate) mod tests;
#[cfg(test)]
#[path = "native_project_close_ui_tests.rs"]
mod ui_tests;
#[path = "native_project_close_ui.rs"]
mod ui;
#[cfg(feature = "validation")]
#[path = "native_project_close_validation.rs"]
mod validation;
#[cfg(feature = "validation")]
#[path = "native_project_close_embedded_validation.rs"]
pub(crate) mod embedded_validation;

use bevy::prelude::*;
pub(crate) use resources::{CloseTarget, NativeProjectCloseState};
pub(crate) use systems::authorize_exit;
#[cfg(feature = "validation")]
pub(crate) use validation::{
    begin as begin_validation, completion as validation_completion, drive as drive_validation,
    install as install_validation,
};

/// 在原作者输入前认领确认层；后续工程菜单只合并这个所有权，不覆盖它。
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ProjectCloseInput;

pub(crate) struct NativeProjectClosePlugin;

/// 宿主取消准备或候选失败时，尚在Draining收尾的关闭也沿原取消入口撤回。
/// AppExit已经排入则退出已经提交，不能再声称返回编辑，也不伪造作者保存。
pub(crate) fn cancel_workspace_close(world: &mut World) {
    if world.get_resource::<Messages<AppExit>>().is_some_and(|exits| !exits.is_empty()) {
        return;
    }
    let mut state = world.resource_mut::<NativeProjectCloseState>();
    if state.active() {
        state.choice = Some(components::CloseAction::Cancel);
    }
}

/// 仅同窗口自建工程或独占无工程验收收尾使用；保存和退出仍等待原业务回执。
/// 失败不自动重试或放弃，普通会话不安装此系统，也不替用户确认未保存内容。
#[cfg(feature = "validation")]
pub(crate) fn confirm_workspace_validation_close(mut state: ResMut<NativeProjectCloseState>) {
    if state.stage == resources::CloseStage::Confirm && state.failed.is_none() {
        let action = if state.needs_save_decision() {
            components::CloseAction::Save
        } else {
            components::CloseAction::Close
        };
        info!("同窗口自有工程验收选择原关闭动作 {action:?}，文档数={}", state.documents.len());
        state.choice = Some(action);
    }
}

impl Plugin for NativeProjectClosePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NativeProjectCloseState>()
            // 无工程主工作台也复用此确认层；取消反馈不依赖作者/运行插件代为注册。
            .add_message::<feathrise_editor_map::standalone::SavedPlayFeedback>()
            .add_systems(Update, ui::mount)
            .add_systems(
                Update,
                ui::handle
                    .in_set(ProjectCloseInput)
                    .before(feathrise_editor_ui::EditorUiSystemSet::NumericInput)
                    .before(feathrise_editor_core::MapEditorSystemSet::Input),
            )
            .add_systems(Update, ui::present.after(crate::native_play::request_exit));
    }
}
