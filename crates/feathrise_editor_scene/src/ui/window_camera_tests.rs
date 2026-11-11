//! 通过Bevy正式UI目标传播验证切换场景各阶段的窗口倍率，既有三维场景只改变Disabled资格。

use super::*;
use bevy::app::{HierarchyPropagatePlugin, PropagateSet};
use bevy::camera::{ComputedCameraValues, RenderTargetInfo};
use bevy::ui::{
    ComputedUiRenderTargetInfo, ComputedUiTargetCamera, update::propagate_ui_target_cameras,
};
use bevy::window::PrimaryWindow;

fn camera(size: UVec2, dpi: f32) -> Camera {
    Camera {
        computed: ComputedCameraValues {
            target_info: Some(RenderTargetInfo {
                physical_size: size,
                scale_factor: dpi,
            }),
            ..default()
        },
        ..default()
    }
}

fn fixture() -> (App, Entity, Entity, Entity) {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(HierarchyPropagatePlugin::<ComputedUiTargetCamera>::new(
            PostUpdate,
        ))
        .add_plugins(HierarchyPropagatePlugin::<ComputedUiRenderTargetInfo>::new(
            PostUpdate,
        ))
        .insert_resource(SceneEditorState {
            open: true,
            ..default()
        })
        .insert_resource(UiScale(0.85));
    app.world_mut().spawn((Window::default(), PrimaryWindow));
    let source = app
        .world_mut()
        .spawn((
            Camera3d::default(),
            camera(UVec2::new(1920, 1080), 1.25),
            IsDefaultUiCamera,
        ))
        .id();
    let target = app
        .world_mut()
        .spawn((
            Camera3d::default(),
            camera(UVec2::new(1280, 720), 1.0),
            IsDefaultUiCamera,
            Disabled,
        ))
        .id();
    let root = app.world_mut().spawn((SceneEditorRoot, Node::default())).id();
    // 固定相机的真实目标信息通常由camera_system更新；无渲染夹具直接注入同一窗口的当前值。
    app.world_mut().spawn((
        SceneEditorUiCamera,
        Camera2d,
        camera(UVec2::new(1920, 1080), 1.25),
        RenderLayers::none(),
        Disabled,
    ));
    app.configure_sets(
        PostUpdate,
        PropagateSet::<ComputedUiTargetCamera>::default().after(propagate_ui_target_cameras),
    );
    app.configure_sets(
        PostUpdate,
        PropagateSet::<ComputedUiRenderTargetInfo>::default().after(propagate_ui_target_cameras),
    );
    app.add_systems(
        PostUpdate,
        propagate_ui_target_cameras.in_set(bevy::ui::UiSystems::Prepare),
    );
    (app, root, source, target)
}

#[test]
fn editor_ui_scale_and_window_size_stay_fixed_during_resident_camera_handoff() {
    let (mut app, root, source, target) = fixture();
    // 不安装修正系统时，正式目标传播会在暂停帧把倍率退回1、窗口尺寸退回0。
    install(&mut app);
    app.update();
    let ui_camera = app
        .world()
        .get::<ComputedUiTargetCamera>(root)
        .unwrap()
        .get()
        .unwrap();
    for (source_enabled, target_enabled) in
        [(false, false), (false, true), (false, false), (true, false)]
    {
        for (entity, enabled) in [(source, source_enabled), (target, target_enabled)] {
            if enabled {
                app.world_mut().entity_mut(entity).remove::<Disabled>();
            } else {
                app.world_mut().entity_mut(entity).insert(Disabled);
            }
        }
        app.update();
        let info = app.world().get::<ComputedUiRenderTargetInfo>(root).unwrap();
        assert_eq!(info.scale_factor(), 0.85 * 1.25);
        assert_eq!(info.physical_size(), UVec2::new(1920, 1080));
        assert_eq!(
            app.world()
                .get::<ComputedUiTargetCamera>(root)
                .unwrap()
                .get(),
            Some(ui_camera)
        );
    }
    assert_eq!(app.world().resource::<UiScale>().0, 0.85);
}

#[test]
fn closing_editor_releases_default_ui_camera_and_reopening_reuses_original_camera() {
    let (mut app, root, source, target) = fixture();
    install(&mut app);
    app.update();
    let ui_camera = app.world().get::<UiTargetCamera>(root).unwrap().entity();
    app.world_mut().entity_mut(source).insert(Disabled);
    app.world_mut().entity_mut(target).insert(Disabled);
    let menu_camera = app
        .world_mut()
        .spawn((Camera3d::default(), camera(UVec2::new(1920, 1080), 1.25)))
        .id();
    let menu_root = app.world_mut().spawn(Node::default()).id();
    for _ in 0..3 {
        app.world_mut().resource_mut::<SceneEditorState>().open = false;
        app.update();
        assert!(!app.world().get::<Camera>(ui_camera).unwrap().is_active);
        assert!(app.world().get::<Disabled>(ui_camera).is_none()); // 首个关闭帧必须仍能被原生提取查询看见。
        app.update(); // 提取已经消费停用状态，下一帧才撤下默认UI候选资格。
        assert!(app.world().get::<Disabled>(ui_camera).is_some());
        assert_eq!(
            app.world()
                .get::<ComputedUiTargetCamera>(menu_root)
                .unwrap()
                .get(),
            Some(menu_camera)
        );
        app.world_mut().resource_mut::<SceneEditorState>().open = true;
        app.update();
        assert!(app.world().get::<Camera>(ui_camera).unwrap().is_active);
        assert!(app.world().get::<Disabled>(ui_camera).is_none());
        assert_eq!(
            app.world().get::<UiTargetCamera>(root).unwrap().entity(),
            ui_camera
        );
        assert_eq!(
            app.world_mut()
                .query_filtered::<Entity, (With<SceneEditorUiCamera>, Allow<Disabled>)>()
                .iter(app.world())
                .count(),
            1
        );
    }
}

#[test]
fn live_window_dpi_size_and_editor_preferences_update_without_changing_ui_camera() {
    let (mut app, root, source, _) = fixture();
    install(&mut app);
    app.update();
    let ui_camera = app.world().get::<UiTargetCamera>(root).unwrap().entity();
    app.world_mut().entity_mut(source).insert(Disabled);
    // 固定的是承载者身份；窗口DPI、尺寸及设置仍采用当前值，不能缓存成首次打开的倍率。
    *app.world_mut().get_mut::<Camera>(ui_camera).unwrap() = camera(UVec2::new(2560, 1440), 1.5);
    app.world_mut().resource_mut::<UiScale>().0 = 1.25;
    app.update();
    let info = app.world().get::<ComputedUiRenderTargetInfo>(root).unwrap();
    assert_eq!(info.scale_factor(), 1.875);
    assert_eq!(info.physical_size(), UVec2::new(2560, 1440));
    assert_eq!(
        app.world().get::<UiTargetCamera>(root).unwrap().entity(),
        ui_camera
    );
}

#[test]
fn new_editor_root_gets_window_camera_before_first_ui_layout() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(SceneEditorState {
            open: false,
            ..default()
        });
    install(&mut app);
    let root = app.world_mut().spawn((SceneEditorRoot, Node::default())).id();
    app.update();
    assert!(app.world().get::<UiTargetCamera>(root).is_none());
    app.world_mut().resource_mut::<SceneEditorState>().open = true;
    app.update();
    let ui_camera = app.world().get::<UiTargetCamera>(root).unwrap().entity();
    assert!(app.world().get::<SceneEditorUiCamera>(ui_camera).is_some());
    assert!(matches!(
        app.world().get::<RenderTarget>(ui_camera).unwrap(),
        RenderTarget::Window(bevy::window::WindowRef::Primary)
    ));
    assert_eq!(
        *app.world().get::<RenderLayers>(ui_camera).unwrap(),
        RenderLayers::none()
    );
    app.world_mut().entity_mut(root).despawn();
    let replacement = app.world_mut().spawn((SceneEditorRoot, Node::default())).id();
    app.update();
    assert_eq!(
        app.world()
            .get::<UiTargetCamera>(replacement)
            .unwrap()
            .entity(),
        ui_camera
    );
}
