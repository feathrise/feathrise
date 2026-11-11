//! 全局与工程插件管理：作者进程负责真实装配，管理界面只提交有界后台安装计划。
pub(crate) mod components;
pub(crate) mod directory;
#[cfg(test)]
mod control_tests;
#[cfg(test)]
mod schedule_tests;
pub(crate) mod packages;
pub(crate) mod resources;
mod systems;
#[cfg(test)]
mod tests;
pub(crate) mod types;
mod ui;
#[cfg(all(test, feature = "validation"))]
pub(crate) mod validation;
#[cfg(feature = "validation")]
pub(crate) mod validation_binary;

use bevy::prelude::*;
pub(crate) use resources::PluginManagerState;

#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct PluginManagerInput;

pub(crate) fn install(app: &mut App, root: std::path::PathBuf) {
    install_state(app, resources::PluginManagerState::new(root));
}

/// 无工程时常驻宿主仍能管理当前用户插件；与作者实例复用同一UI和后台事务。
pub(crate) fn install_host(app: &mut App) {
    install_state(app, resources::PluginManagerState::for_global());
}

fn install_state(app: &mut App, state: resources::PluginManagerState) {
    // 每个App只安装一次。主宿主和独立作者拥有不同World，不共享可变管理状态。
    app.insert_resource(state)
        .add_systems(Update, ui::mount.before(ui::present))
        .add_systems(
            Update,
            (ui::scroll, systems::control)
                .chain()
                .in_set(PluginManagerInput)
                // 诊断输入在视口、代码迭代之后；管理器先于视口认领模态，不能反向等待诊断形成环。
                .after(crate::native_project_close::ProjectCloseInput)
                .after(crate::project_log::ProjectLogInput)
                .before(feathrise_editor_ui::EditorUiSystemSet::NumericInput)
                .before(feathrise_editor_core::SceneEditorSystemSet::Input),
        )
        .add_systems(
            Update,
            (systems::poll, ui::present, ui::scrollbar_feedback)
                .chain()
                .after(PluginManagerInput)
                // 公共按钮先更新颜色，管理器随后统一校正禁用/范围状态，避免写入顺序互相覆盖。
                .after(feathrise_editor_ui::button_color_system),
        );
}

/// 有工程由当前作者打开，无工程由主宿主打开全局范围；不隐式打开另一个工程。
pub(crate) fn open_from_workspace(world: &mut World) {
    if world
        .get_resource::<crate::native_project_close::NativeProjectCloseState>()
        .is_some_and(|state| state.captures_input())
        || crate::project_export::ui::captures_input(world)
    {
        return;
    }
    if world.get_resource::<crate::workspace_ui::AboutState>()
        .is_some_and(|state| state.captures_input()) {
        return;
    }
    if let Some(mut state) = world.get_resource_mut::<resources::PluginManagerState>() {
        state.open = true;
        state.revision = state.revision.wrapping_add(1);
    }
}

/// 关闭已经明确授权后才取消本管理器自己的目录选择器；写入计划的任务必须真正收口。
pub(crate) fn ready_to_exit(world: &mut World) -> bool {
    if let Some(picker) = world.get_non_send::<resources::PluginPackagePicker>() {
        picker.picker.cancel();
        return false;
    }
    world
        .get_resource::<resources::PluginManagerState>()
        .is_none_or(|state| !state.busy)
}

#[cfg(test)]
mod export_capture_tests {
    use super::*;

    #[test]
    fn export_capture_rejects_management_open_and_drops_stale_actions_before_reopen() {
        let mut world = World::new();
        let mut plugins = PluginManagerState::new("unused-owned-project".into());
        plugins.refreshed = true; // 只检查输入归属，不创建扫描、写入或原生目录任务。
        world.insert_resource(plugins);
        let mut export = crate::project_export::ui::ExportOverlayState::default();
        export.captured = true;
        world.insert_resource(export);
        open_from_workspace(&mut world);
        assert!(!world.resource::<PluginManagerState>().open, "导出打开及关闭释放帧不能从工作台重开插件管理");
        world.resource_mut::<PluginManagerState>().open = true;
        let action = world.spawn((components::ManagerAction::Refresh, feathrise_editor_ui::EditorButtonClick::triggered())).id();
        systems::control(&mut world);
        assert!(!world.resource::<PluginManagerState>().open);
        assert!(!world.get::<feathrise_editor_ui::EditorButtonClick>(action).unwrap().is_clicked());
        world.resource_mut::<crate::project_export::ui::ExportOverlayState>().captured = false;
        open_from_workspace(&mut world);
        systems::control(&mut world);
        let plugins = world.resource::<PluginManagerState>();
        assert!(plugins.open);
        assert!(!plugins.busy && plugins.task.is_none() && plugins.sequence == 0,
            "重开后不能把另一浮窗期间的旧刷新回执变成后台扫描");
    }
}
