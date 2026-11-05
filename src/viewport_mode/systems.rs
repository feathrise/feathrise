//! 模式变更先经过原保存、工程运行和关闭门，再发布有限状态；后台生命周期不由 UI 猜测。

use super::{
    RequestViewportMode, ViewportModeAvailability, ViewportModeChanged, ViewportModeState,
};
use bevy::prelude::*;
use feathrise_editor_map::standalone::SavedPlayFeedback;

pub(super) fn apply(
    world: &mut World,
    mut cursor: Local<bevy::ecs::message::MessageCursor<RequestViewportMode>>,
) {
    let Some(request) = cursor
        .read(world.resource::<Messages<RequestViewportMode>>())
        .last()
        .copied()
    else {
        return;
    };
    if request.0 == world.resource::<ViewportModeState>().selected {
        return;
    }
    // 首次工程3D编译尚未就绪时，切回也是明确的停止请求；不能要求作者先等编译结束。
    if request.0 == super::ViewportMode::SavedWorld && super::compilation::stop_and_return(world) {
        return;
    }
    if let Some(reason) = blocked_reason(world, request.0) {
        world.resource_mut::<ViewportModeState>().status = reason.clone();
        world.write_message(SavedPlayFeedback(reason));
        return;
    }
    let mut state = world.resource_mut::<ViewportModeState>();
    state.selected = request.0;
    state.revision = state.revision.wrapping_add(1).max(1);
    state.status = match request.0 {
        super::ViewportMode::SavedWorld => "已切换到编辑器 3D；点击“启动”运行已保存的工程".into(),
        super::ViewportMode::EmbeddedWorld => "已切换到工程3D；正在准备常驻视口".into(),
    };
    let feedback = state.status.clone();
    if let Some(mut launch) = world.get_resource_mut::<feathrise_editor_core::ProjectLaunchSettings>() {
        launch.preview = request.0 == super::ViewportMode::EmbeddedWorld;
    }
    world.write_message(ViewportModeChanged);
    world.write_message(SavedPlayFeedback(feedback));
}

/// 配置只在安全编辑状态受理；常驻协调器随后停止旧Job，真实回收后按新配置重建。
pub(super) fn apply_profile(
    world: &mut World,
    mut cursor: Local<bevy::ecs::message::MessageCursor<super::messages::RequestBuildProfile>>,
) {
    let Some(request) = cursor.read(world.resource::<Messages<super::messages::RequestBuildProfile>>()).last().copied() else {
        return;
    };
    if request.0 == world.resource::<feathrise_editor_core::ProjectLaunchSettings>().profile {
        return;
    }
    let mode = world.resource::<ViewportModeState>().selected;
    let reason = crate::native_play::build_profile_blocker(world)
        .or_else(|| crate::project_export::build_profile_blocker(world))
        .or_else(|| crate::embedded_viewport::build_profile_blocker(world))
        .or_else(|| blocked_reason(world, mode));
    if let Some(reason) = reason {
        world.resource_mut::<ViewportModeState>().status = reason.clone();
        world.write_message(SavedPlayFeedback(reason));
        return; // 不排队旧点击；结束工程运行/关闭后必须由作者重新选择。
    }
    let restarting = mode == super::ViewportMode::EmbeddedWorld
        && world.get_resource::<crate::embedded_viewport::EmbeddedStatus>().is_some_and(|status| status.active);
    let mut launch = world.resource_mut::<feathrise_editor_core::ProjectLaunchSettings>();
    launch.profile = request.0;
    launch.switching_profile = restarting;
    let message = if mode == super::ViewportMode::EmbeddedWorld {
        format!("已选择{}预览；正在{}，作者草稿和历史保留", request.0.label(),
            if restarting { "收口当前工程实例并重新构建" } else { "准备所选配置" })
    } else {
        format!("已选择{}；下次启动使用此配置", request.0.label())
    };
    world.resource_mut::<ViewportModeState>().status = message.clone();
    world.write_message(SavedPlayFeedback(message));
}

pub(super) fn blocked_reason(world: &World, requested: super::ViewportMode) -> Option<String> {
    if !requested.supported() {
        return Some("当前系统使用编辑器3D；点击“启动”运行已保存的工程".into());
    }
    if world.get_resource::<crate::workspace_ui::AboutState>().is_some_and(|about| about.captures_input()) {
        return Some("请先关闭关于窗口，再切换视口".into());
    }
    if world.get_resource::<crate::native_plugins::management::PluginManagerState>()
        .is_some_and(|plugins| plugins.captures_input()) {
        return Some("请先关闭插件管理器，再切换3D方式或启动配置".into());
    }
    if crate::project_export::ui::captures_input(world) {
        return Some("请先关闭导出面板，再切换3D方式或启动配置".into());
    }
    if requested == super::ViewportMode::EmbeddedWorld
        && let Some(error) = world
        .get_resource::<feathrise_editor_map::standalone::viewport_projection::ViewportHostProjection>()
        .and_then(|projection| projection.error.as_ref())
    {
        return Some(error.clone());
    }
    if let Some(reason) = world
        .resource::<ViewportModeAvailability>()
        .blocked_reason
        .as_ref()
    {
        return Some(reason.clone());
    }
    if world
        .get_resource::<crate::native_play::NativePlayStatus>()
        .is_some_and(|play| play.active)
    {
        return Some("请先结束当前工程运行或构建，再切换 3D 方式；作者现场保留".into());
    }
    if world
        .get_resource::<crate::native_project_close::NativeProjectCloseState>()
        .is_some_and(|close| close.active())
    {
        return Some("请先完成或取消关闭工程，再切换 3D 方式".into());
    }
    if world
        .get_resource::<crate::native_project_menu::NativeProjectMenuState>()
        .is_some_and(|menu| menu.saving())
        || world
            .get_resource::<crate::workspace_ui::WorkspaceModel>()
            .is_some_and(|workspace| workspace.busy)
        || feathrise_editor_map::standalone::viewport_projection::switch_pending(world)
    {
        return Some("工程操作正在进行，请完成后再切换 3D 方式".into());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewport_mode::ViewportMode;

    fn app() -> App {
        let mut app = App::new();
        app.init_resource::<ViewportModeState>()
            .init_resource::<ViewportModeAvailability>()
            .add_message::<RequestViewportMode>()
            .add_message::<ViewportModeChanged>()
            .add_message::<SavedPlayFeedback>()
            .add_systems(Update, apply);
        app
    }

    #[test]
    #[cfg(windows)]
    fn plugin_manager_and_its_release_frame_consume_mode_requests_without_replay() {
        let mut app = app();
        app.insert_resource(crate::native_plugins::management::PluginManagerState::new("unused-plugin-gate-fixture".into()));
        app.world_mut().resource_mut::<crate::native_plugins::management::PluginManagerState>().open = true;
        app.world_mut().write_message(RequestViewportMode(ViewportMode::EmbeddedWorld));
        app.update();
        assert_eq!(app.world().resource::<ViewportModeState>().selected, ViewportMode::SavedWorld);

        {
            let mut state = app.world_mut().resource_mut::<crate::native_plugins::management::PluginManagerState>();
            state.open = false;
            state.captured = true;
        }
        app.world_mut().write_message(RequestViewportMode(ViewportMode::EmbeddedWorld));
        app.update();
        assert_eq!(app.world().resource::<ViewportModeState>().selected, ViewportMode::SavedWorld);
        app.world_mut().resource_mut::<crate::native_plugins::management::PluginManagerState>().captured = false;
        app.update();
        assert_eq!(app.world().resource::<ViewportModeState>().selected, ViewportMode::SavedWorld,
            "被模态或释放帧消费的旧请求不能在管理器关闭后自动生效");
        app.world_mut().write_message(RequestViewportMode(ViewportMode::EmbeddedWorld));
        app.update();
        assert_eq!(app.world().resource::<ViewportModeState>().selected, ViewportMode::EmbeddedWorld);
    }

    #[test]
    #[cfg(windows)]
    fn a_busy_switch_is_consumed_and_cannot_replay_after_cleanup() {
        let mut app = app();
        app.world_mut()
            .resource_mut::<ViewportModeAvailability>()
            .blocked_reason = Some("正在收口".into());
        app.world_mut()
            .write_message(RequestViewportMode(ViewportMode::EmbeddedWorld));
        app.update();
        assert_eq!(
            app.world().resource::<ViewportModeState>().selected,
            ViewportMode::SavedWorld
        );
        app.world_mut()
            .resource_mut::<ViewportModeAvailability>()
            .blocked_reason = None;
        app.update();
        assert_eq!(
            app.world().resource::<ViewportModeState>().selected,
            ViewportMode::SavedWorld
        );
        app.world_mut()
            .write_message(RequestViewportMode(ViewportMode::EmbeddedWorld));
        app.update();
        assert_eq!(
            app.world().resource::<ViewportModeState>().selected,
            ViewportMode::EmbeddedWorld
        );
        assert_eq!(app.world().resource::<ViewportModeState>().revision, 2);
    }

    #[test]
    fn repeated_selection_does_not_restart_the_runtime() {
        let mut app = app();
        app.world_mut()
            .write_message(RequestViewportMode(ViewportMode::SavedWorld));
        app.update();
        assert_eq!(app.world().resource::<ViewportModeState>().revision, 1);
        assert!(
            app.world()
                .resource::<Messages<ViewportModeChanged>>()
                .is_empty()
        );
    }

    #[test]
    #[cfg(not(windows))]
    fn unsupported_embedded_request_preserves_real_editor_mode_and_does_not_publish_change() {
        let mut app = app();
        app.world_mut().write_message(RequestViewportMode(ViewportMode::EmbeddedWorld));
        app.update();
        let state = app.world().resource::<ViewportModeState>();
        assert_eq!(state.selected, ViewportMode::SavedWorld);
        assert!(state.status.contains("启动"));
        assert!(app.world().resource::<Messages<ViewportModeChanged>>().is_empty());
        app.update();
        assert_eq!(app.world().resource::<ViewportModeState>().revision, 1);
    }

    #[test]
    fn invalid_child_holes_block_entry_but_preserve_return_to_saved_world() {
        use feathrise_editor_map::standalone::viewport_projection::ViewportHostProjection;
        let mut app = app();
        app.insert_resource(ViewportHostProjection {
            error: Some("浮动控件超过窗口区域预算".into()),
            ..default()
        });
        app.world_mut()
            .write_message(RequestViewportMode(ViewportMode::EmbeddedWorld));
        app.update();
        assert_eq!(
            app.world().resource::<ViewportModeState>().selected,
            ViewportMode::SavedWorld
        );
        app.world_mut().resource_mut::<ViewportModeState>().selected = ViewportMode::EmbeddedWorld;
        app.world_mut()
            .write_message(RequestViewportMode(ViewportMode::SavedWorld));
        app.update();
        assert_eq!(
            app.world().resource::<ViewportModeState>().selected,
            ViewportMode::SavedWorld
        );
    }
}
