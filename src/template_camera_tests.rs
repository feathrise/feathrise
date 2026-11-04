//! 直接加载新建模板并驱动 Bevy 原生自由相机，验证玩法许可、焦点及驻留观察点。

#[path = "../assets/templates/empty/src/controller.rs"]
#[allow(unexpected_cfgs)] // 直接复用模板源码；function_hotpatch属于模板清单，不是宿主能力。
mod controller;

use bevy::{
    camera_controller::free_camera::{FreeCamera, FreeCameraState},
    input::{ButtonState, InputPlugin, mouse::{MouseButtonInput, MouseMotion}},
    prelude::*,
    time::TimeUpdateStrategy,
    window::{CursorGrabMode, CursorOptions, PrimaryWindow},
};
use feathrise_runtime::{GameView, project::{PrimarySceneView, ProjectViewOrigin, SceneRuntimeObserver}};
use feathrise_runtime::project::embedded::EmbeddedProjectState;
use feathrise_editor_protocol::Mode;
use std::time::Duration;

fn fixture(view: GameView, mode: Option<Mode>) -> (App, Entity, Entity) {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, InputPlugin))
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(50)))
        .insert_resource(ProjectViewOrigin(Vec3::new(12., 3., 18.)))
        .add_plugins(controller::ProjectScenePlugin { view });
    if let Some(mode) = mode {
        app.insert_resource(EmbeddedProjectState { mode, world_id: 1, revision: 1, transitioning: false });
    }
    let window = app.world_mut().spawn((PrimaryWindow, Window::default(), CursorOptions::default())).id();
    // 地图拥有这唯一一台相机；测试不安装渲染器，原生控制器仍使用真实 Camera 查询和输入资源。
    let camera = app.world_mut().spawn((PrimarySceneView, Camera::default(), Transform::default())).id();
    app.update();
    app.update();
    (app, camera, window)
}

fn right_mouse(app: &mut App, window: Entity) {
    app.world_mut().write_message(MouseButtonInput { button: MouseButton::Right,
        state: ButtonState::Pressed, window });
}

#[test]
fn template_free_camera_moves_both_views_and_updates_the_real_residency_observer() {
    for view in [GameView::ThirdPerson, GameView::FixedTopDown] {
        let (mut app, camera, window) = fixture(view, None);
        assert!(app.world().get::<FreeCamera>(camera).is_some());
        let before = *app.world().get::<Transform>(camera).unwrap();
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyW);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyE);
        right_mouse(&mut app, window);
        app.world_mut().write_message(MouseMotion { delta: Vec2::new(12., -4.) });
        app.update();
        let after = *app.world().get::<Transform>(camera).unwrap();
        assert_ne!(after.translation, before.translation, "{view:?} 的原生 WASD/QE 应能移动");
        assert_ne!(after.rotation, before.rotation, "右键按住时使用原生鼠标环视");
        let observer = app.world_mut().query_filtered::<&Transform, With<SceneRuntimeObserver>>()
            .single(app.world()).unwrap();
        assert_eq!(observer.translation, after.translation);
        assert_eq!(app.world_mut().query_filtered::<Entity, With<Camera>>().iter(app.world()).count(), 1);

        app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
        app.update();
        assert_eq!(*app.world().get::<Transform>(camera).unwrap(), after, "失焦后按住移动键也不得继续滑动");
        let cursor = app.world().get::<CursorOptions>(window).unwrap();
        assert_eq!(cursor.grab_mode, CursorGrabMode::None);
        assert!(cursor.visible);
        assert_eq!(app.world().get::<FreeCameraState>(camera).unwrap().velocity, Vec3::ZERO);
    }
}

#[test]
fn template_free_camera_editing_releases_input_and_resume_uses_the_restored_game_pose() {
    let (mut app, camera, window) = fixture(GameView::ThirdPerson, Some(Mode::Playing));
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyW);
    right_mouse(&mut app, window);
    app.update();
    assert_eq!(app.world().get::<CursorOptions>(window).unwrap().grab_mode, CursorGrabMode::Locked);
    assert_ne!(app.world().get::<FreeCameraState>(camera).unwrap().velocity, Vec3::ZERO);
    let saved_game = *app.world().get::<Transform>(camera).unwrap();
    app.world_mut().resource_mut::<EmbeddedProjectState>().mode = Mode::Editing;
    app.update();
    app.update();
    assert_eq!(*app.world().get::<Transform>(camera).unwrap(), saved_game);
    assert!(!app.world().get::<FreeCameraState>(camera).unwrap().enabled);
    assert_eq!(app.world().get::<CursorOptions>(window).unwrap().grab_mode, CursorGrabMode::None);
    assert!(app.world().get::<CursorOptions>(window).unwrap().visible);

    // 模拟 SDK 在编辑期借用相机、F12 开门前归还游戏姿态并清掉作者输入；
    // 原生插件必须重新从该游戏姿态初始化，不能沿用作者姿态或旧的移动惯性。
    let author = Transform::from_xyz(-90., 60., 120.).looking_at(Vec3::ZERO, Vec3::Y);
    *app.world_mut().get_mut::<Transform>(camera).unwrap() = author;
    app.update();
    assert_eq!(*app.world().get::<Transform>(camera).unwrap(), author);
    let resumed_game = saved_game.with_rotation(Quat::from_euler(EulerRot::YXZ, 0.8, -0.2, 0.));
    *app.world_mut().get_mut::<Transform>(camera).unwrap() = resumed_game;
    app.world_mut().get_mut::<FreeCameraState>(camera).unwrap().speed_multiplier = 2.5;
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().reset_all();
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().reset_all();
    app.world_mut().resource_mut::<EmbeddedProjectState>().mode = Mode::Playing;
    app.update();
    assert_eq!(*app.world().get::<Transform>(camera).unwrap(), resumed_game);
    assert_eq!(app.world().get::<FreeCameraState>(camera).unwrap().speed_multiplier, 2.5);
    right_mouse(&mut app, window);
    app.world_mut().write_message(MouseMotion { delta: Vec2::new(9., 0.) });
    app.update();
    let rotation = app.world().get::<Transform>(camera).unwrap().rotation;
    let expected = Quat::from_euler(EulerRot::YXZ, 0.79, -0.2, 0.);
    assert!(rotation.angle_between(expected) < 0.001, "恢复后的首次环视不得跳到作者相机方向");
}
