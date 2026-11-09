//! DX/原生选择通过原菜单输入与业务门验证；不创建用户工程或伪造编译成功。

use super::{components::*, messages::*, resources::BuildProfileMenuState, *};
use bevy::{input_focus::InputFocus, prelude::*};
use crate::native_play::ProjectBuildBackend;
use feathrise_editor_core::ProjectLaunchSettings;
use feathrise_editor_ui::EditorButtonClick;

fn app() -> App {
    let mut app = App::new();
    app.init_resource::<ViewportModeState>().init_resource::<ViewportModeAvailability>()
        .init_resource::<ProjectLaunchSettings>().init_resource::<ProjectBuildBackend>()
        .init_resource::<BuildProfileMenuState>().init_resource::<InputFocus>()
        .init_resource::<ButtonInput<MouseButton>>().init_resource::<ButtonInput<KeyCode>>()
        .add_message::<RequestBuildProfile>().add_message::<RequestBuildBackend>()
        .add_message::<RequestViewportMode>().add_message::<ViewportModeChanged>()
        .add_message::<feathrise_editor_map::standalone::SavedPlayFeedback>()
        .add_systems(Update, (super::profile_ui::input, super::systems::apply, super::systems::apply_backend, super::profile_ui::present).chain());
    let font = Handle::<Font>::default();
    let mut commands = app.world_mut().commands();
    commands.spawn(Node::default()).with_children(|header| super::profile_ui::mount(header, &font));
    app.world_mut().flush();
    app
}

fn click(app: &mut App, entity: Entity) {
    app.world_mut().entity_mut(entity).insert(EditorButtonClick::triggered());
    app.update();
    app.world_mut().entity_mut(entity).insert(EditorButtonClick::default());
    app.update();
}

fn select(app: &mut App, backend: ProjectBuildBackend) {
    let world = app.world_mut();
    let toggle = world.query_filtered::<Entity, With<ProfileToggle>>().single(world).unwrap();
    let option = world.query::<(Entity, &BackendOption)>().iter(world).find(|(_, option)| option.0 == backend).unwrap().0;
    click(app, toggle);
    click(app, option);
}

#[test]
fn editor_3d_menu_defaults_to_dx_and_changes_only_the_next_launch_backend() {
    let mut app = app();
    app.update();
    assert_eq!(*app.world().resource::<ProjectBuildBackend>(), ProjectBuildBackend::Dx);
    let group = { let world=app.world_mut(); world.query_filtered::<Entity, With<BackendOptions>>().single(world).unwrap() };
    assert_eq!(app.world().get::<Node>(group).unwrap().display, Display::Flex);
    select(&mut app, ProjectBuildBackend::Native);
    assert_eq!(*app.world().resource::<ProjectBuildBackend>(), ProjectBuildBackend::Native);
    assert_eq!(ProjectBuildBackend::effective(app.world()), ProjectBuildBackend::Native);
    assert_eq!(app.world().resource::<ViewportModeState>().selected, ViewportMode::SavedWorld);
    assert_eq!(app.world().resource::<ViewportModeState>().revision, 1, "编辑器3D选择工具不能触发工程预览重建");
    assert!(!app.world().resource::<ProjectLaunchSettings>().switching_profile);
    assert!(app.world().resource::<ViewportModeState>().status.contains("下次启动"));
    let world = app.world_mut();
    let title = world.query::<(&ProfileLabel, &Text)>().iter(world).find(|(label,_)|label.0.is_none()).unwrap().1;
    assert_eq!(title.as_str(), "Debug · Cargo ▼");
    select(&mut app, ProjectBuildBackend::Native);
    assert_eq!(app.world().resource::<ViewportModeState>().revision, 1, "重复选择不重启或换代");
    select(&mut app, ProjectBuildBackend::Dx);
    assert_eq!(*app.world().resource::<ProjectBuildBackend>(), ProjectBuildBackend::Dx);
    assert_eq!(app.world().resource::<ViewportModeState>().revision, 1);
}

#[test]
#[cfg(windows)]
fn shared_backend_survives_mode_round_trips_and_only_rebuilds_project_3d() {
    let mut app = app();
    select(&mut app, ProjectBuildBackend::Native);
    app.world_mut().write_message(RequestViewportMode(ViewportMode::EmbeddedWorld));
    app.update();
    assert_eq!(app.world().resource::<ViewportModeState>().selected, ViewportMode::EmbeddedWorld);
    assert_eq!(app.world().resource::<ViewportModeState>().revision, 2);
    assert_eq!(ProjectBuildBackend::effective(app.world()), ProjectBuildBackend::Native);
    select(&mut app, ProjectBuildBackend::Dx);
    assert_eq!(ProjectBuildBackend::effective(app.world()), ProjectBuildBackend::Dx);
    assert_eq!(app.world().resource::<ViewportModeState>().revision, 3);
    select(&mut app, ProjectBuildBackend::Native);
    assert_eq!(app.world().resource::<ViewportModeState>().revision, 4);
    select(&mut app, ProjectBuildBackend::Native);
    assert_eq!(app.world().resource::<ViewportModeState>().revision, 4, "重复选择不能多排一次构建");
    app.world_mut().write_message(RequestViewportMode(ViewportMode::SavedWorld));
    app.update();
    assert_eq!(app.world().resource::<ViewportModeState>().selected, ViewportMode::SavedWorld);
    assert_eq!(ProjectBuildBackend::effective(app.world()), ProjectBuildBackend::Native, "切回编辑器3D不能暗中恢复DX");
}

#[test]
fn blocked_backend_click_is_consumed_and_requires_a_fresh_selection() {
    let mut app = app();
    app.world_mut().resource_mut::<ViewportModeState>().selected = ViewportMode::EmbeddedWorld;
    app.world_mut().resource_mut::<ViewportModeAvailability>().blocked_reason = Some("等待旧任务收口".into());
    select(&mut app, ProjectBuildBackend::Native);
    assert_eq!(*app.world().resource::<ProjectBuildBackend>(), ProjectBuildBackend::Dx);
    app.world_mut().resource_mut::<ViewportModeAvailability>().blocked_reason = None;
    app.update();
    assert_eq!(*app.world().resource::<ProjectBuildBackend>(), ProjectBuildBackend::Dx);
    select(&mut app, ProjectBuildBackend::Native);
    assert_eq!(*app.world().resource::<ProjectBuildBackend>(), ProjectBuildBackend::Native);
}

#[test]
fn active_standalone_run_rejects_backend_selection_without_replaying_the_click() {
    let mut app = app();
    app.insert_resource(crate::native_play::NativePlayStatus { active: true, ..default() });
    select(&mut app, ProjectBuildBackend::Native);
    assert_eq!(*app.world().resource::<ProjectBuildBackend>(), ProjectBuildBackend::Dx);
    assert!(!app.world().resource::<BuildProfileMenuState>().open);
    app.world_mut().resource_mut::<crate::native_play::NativePlayStatus>().active = false;
    app.update();
    assert_eq!(*app.world().resource::<ProjectBuildBackend>(), ProjectBuildBackend::Dx, "运行收口不能重放被拒的后端点击");
    select(&mut app, ProjectBuildBackend::Native);
    assert_eq!(*app.world().resource::<ProjectBuildBackend>(), ProjectBuildBackend::Native);
}
