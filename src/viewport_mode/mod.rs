//! 正式作者视口双模式：原作者 World 与常驻工程实例 World 保留为可对照的用户选择。
//! 此模块只拥有选择、用户偏好和中央窗口几何；构建、IPC 与游戏生命周期由宿主桥接层负责。

mod components;
mod compilation;
mod busy_border;
mod messages;
mod preferences;
mod profile_ui;
mod resources;
mod systems;
mod types;
mod ui;
#[cfg(test)]
mod ui_tests;
#[cfg(all(test, windows))]
mod backend_tests;
#[cfg(all(test, windows))]
mod gpu_tests;
pub(crate) mod window;

pub(crate) use messages::{RequestViewportMode, ViewportModeChanged};
pub(crate) use preferences::{Completion, ready_to_exit};
pub(crate) use resources::{BuildProfileMenuState, ViewportGeometry, ViewportModeAvailability, ViewportModeState};
pub(crate) use types::ViewportMode;

use bevy::prelude::*;

/// 生命周期系统应在 Input 之后消费模式，几何在 PostUpdate 布局后发布供下一帧合并发送。
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ViewportModeInput;

pub(crate) fn install(
    app: &mut App,
    preferences: &crate::user_preferences::Preferences,
) -> Completion {
    feathrise_editor_map::standalone::viewport_projection::install(app);
    let completion = self::preferences::install(app, preferences);
    {
        let mut mode = app.world_mut().resource_mut::<ViewportModeState>();
        if !mode.selected.supported() {
            // 保留偏好原文身份，由原Last后台写者持久化归一化结果，不在安装帧写磁盘。
            mode.selected = ViewportMode::SavedWorld;
            mode.revision = mode.revision.wrapping_add(1).max(1);
            mode.status = "已切换到编辑器 3D；点击“启动”运行已保存的工程".into();
        }
    }
    let preview = app.world().resource::<ViewportModeState>().selected == ViewportMode::EmbeddedWorld;
    app.init_resource::<feathrise_editor_core::ProjectLaunchSettings>();
    app.world_mut().resource_mut::<feathrise_editor_core::ProjectLaunchSettings>().preview = preview;
    app.init_resource::<ViewportModeAvailability>()
        .init_resource::<resources::CompilationReturn>()
        .init_resource::<feathrise_editor_core::ProjectLaunchInterruption>()
        .init_resource::<BuildProfileMenuState>()
        .init_resource::<crate::native_play::ProjectBuildBackend>()
        .init_resource::<ViewportGeometry>()
        .add_message::<RequestViewportMode>()
        .add_message::<ViewportModeChanged>()
        .add_message::<messages::RequestBuildProfile>()
        .add_message::<messages::RequestBuildBackend>()
        .add_message::<feathrise_editor_map::standalone::SavedPlayFeedback>()
        .add_systems(Update, ui::mount.before(ViewportModeInput))
        .add_systems(Update, ui::mount_launch.before(ViewportModeInput))
        .add_systems(Update, compilation::input
            .after(feathrise_editor_ui::update_editor_button_clicks)
            .after(crate::native_project_close::ProjectCloseInput)
            .after(crate::project_log::ProjectLogInput)
            .after(crate::native_plugins::management::PluginManagerInput)
            .after(crate::project_export::ExportInput)
            .before(feathrise_editor_core::MapEditorSystemSet::Input)
            .before(ViewportModeInput))
        .add_systems(
            Update,
            (ui::input, profile_ui::input, compilation::return_when_stopped, systems::apply, systems::apply_profile, systems::apply_backend)
                .chain()
                .in_set(ViewportModeInput)
                .after(feathrise_editor_ui::update_editor_button_clicks)
                .after(crate::native_project_close::ProjectCloseInput)
                .after(crate::project_log::ProjectLogInput)
                .after(crate::native_plugins::management::PluginManagerInput)
                .before(crate::native_play::request_exit),
        )
        .add_systems(Update, ui::present.after(ViewportModeInput)
            .after(feathrise_editor_ui::EditorUiSystemSet::NumericInput))
        .add_systems(Update, profile_ui::present.after(ViewportModeInput))
        .add_systems(PostUpdate, (ui::present_connection, ui::present_launch, compilation::present, busy_border::animate).chain()
            .in_set(feathrise_editor_ui::feedback::PanelStatusProjection)
            .before(bevy::ui::UiSystems::Prepare))
        .add_systems(
            PostUpdate,
            window::measure.after(
                feathrise_editor_map::standalone::viewport_projection::ViewportProjectionSet,
            ).after(crate::window_chrome::ChromeMeasure),
        );
    completion
}

/// 核心生命周期回归只安装真实输入路由，不公开私有组件或启动完整作者窗口。
#[cfg(test)]
pub(crate) fn compilation_input_test_schedule(world: &mut World) -> (Schedule, Entity) {
    world.init_resource::<resources::CompilationReturn>();
    world.init_resource::<Messages<RequestViewportMode>>();
    world.init_resource::<Messages<ViewportModeChanged>>();
    world.init_resource::<Messages<feathrise_editor_map::standalone::SavedPlayFeedback>>();
    world.init_resource::<feathrise_editor_core::ProjectLaunchSettings>();
    let mode_button = world.spawn((
        Button, components::ModeButton(ViewportMode::SavedWorld),
        feathrise_editor_ui::EditorButtonClick::default(), InheritedVisibility::VISIBLE,
    )).id();
    let mut schedule = Schedule::default();
    schedule.add_systems(compilation::input.before(ViewportModeInput));
    schedule.add_systems((ui::input, compilation::return_when_stopped, systems::apply)
        .chain().in_set(ViewportModeInput));
    (schedule, mode_button)
}
