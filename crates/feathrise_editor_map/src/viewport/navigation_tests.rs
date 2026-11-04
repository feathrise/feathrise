//! 使用真实视口输入系统验证四个入口的旋转、平移、缩放及模态隔离，不启动窗口或GPU。

use super::*;
use crate::types::MapEditorViewSnapshot;
use bevy::input::mouse::MouseScrollUnit;

fn viewport_state() -> MapEditorViewportState {
    MapEditorViewportState {
        _image: Handle::default(),
        camera: Entity::PLACEHOLDER,
        center: Vec3::new(10.0, 200.0, 30.0),
        scene_center: Vec3::new(-100.0, 20.0, 300.0),
        scene_half_extents: Vec3::new(500.0, 80.0, 200.0),
        extent: 120.0,
        yaw: 2.0,
        pitch: -0.5,
        framed_map_id: None,
        render_size: UVec2::new(600, 400),
    } // 深度分量刻意不同，能检出导航意外重置焦点或缩放。
}

fn fixture(view: EditorView) -> App {
    let mut app = App::new();
    let mut editor = MapEditorState {
        open: true,
        ..default()
    };
    let mut viewport = viewport_state();
    set_editor_view(&mut editor, Some(&mut viewport), view);
    app.insert_resource(editor)
        .insert_resource(viewport)
        .init_resource::<MapEditorNumericScrubMouseGuard>()
        .init_resource::<WaterNetworkEditState>()
        .init_resource::<EditorAuthoringSession>()
        .init_resource::<ButtonInput<MouseButton>>()
        .add_message::<MouseMotion>()
        .add_message::<MouseWheel>()
        .add_systems(Update, control_editor_viewport);
    app.world_mut().spawn((
        MapEditorViewport,
        RelativeCursorPosition {
            cursor_over: true,
            normalized: Some(Vec2::ZERO),
        },
    ));
    app
}

fn snapshot(app: &App) -> MapEditorViewSnapshot {
    let viewport = app.world().resource::<MapEditorViewportState>();
    MapEditorViewSnapshot {
        view: app.world().resource::<MapEditorState>().view,
        orthographic: app.world().resource::<MapEditorState>().orthographic,
        center: viewport.center,
        extent: viewport.extent,
        yaw: viewport.yaw,
        pitch: viewport.pitch,
    }
}

fn drag(app: &mut App, button: MouseButton, delta: Vec2) {
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(button);
    app.world_mut().write_message(MouseMotion { delta });
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .reset_all(); // 同步模拟松手；clear只清边沿，不能让上一笔右键串入下一笔中键平移。
}

#[test]
fn right_drag_from_every_view_orbits_without_dirtying_the_map() {
    for view in [
        EditorView::Perspective,
        EditorView::Top,
        EditorView::Front,
        EditorView::Side,
    ] {
        let mut app = fixture(view);
        let before = snapshot(&app);
        let revision = app.world().resource::<EditorAuthoringSession>().revision;
        drag(&mut app, MouseButton::Right, Vec2::new(12.0, -8.0));
        let after = snapshot(&app);
        assert_eq!(after.view, EditorView::Perspective);
        assert_eq!(after.center, before.center); // 方向按钮已对准；起转不再重设焦点或把缩放换成另一套单位。
        assert_eq!(after.extent, before.extent);
        assert_eq!(after.orthographic, before.orthographic);
        assert!(
            (after.yaw - (before.yaw - 12.0 * 0.006).rem_euclid(std::f32::consts::TAU)).abs()
                < 0.001
        );
        assert!((after.pitch - before.pitch + 8.0 * 0.006).abs() < 0.001);
        assert!(!app.world().resource::<MapEditorState>().dirty);
        assert_eq!(
            app.world().resource::<EditorAuthoringSession>().revision,
            revision
        ); // 旋转属于观察状态，不能制造未保存切图提示。
    }
}

#[test]
fn right_press_without_motion_keeps_fixed_direction() {
    for view in [EditorView::Top, EditorView::Front, EditorView::Side] {
        let mut app = fixture(view);
        let before = snapshot(&app);
        drag(&mut app, MouseButton::Right, Vec2::ZERO);
        assert_eq!(snapshot(&app), before); // 右键只有电平而没有拖动时不擅自换视图。
    }
}

#[test]
fn orthographic_navigation_keeps_projection_from_all_direction_shortcuts() {
    for view in [
        EditorView::Perspective,
        EditorView::Top,
        EditorView::Front,
        EditorView::Side,
    ] {
        let mut app = fixture(view);
        app.world_mut()
            .resource_mut::<MapEditorState>()
            .orthographic = true;
        let before = snapshot(&app);
        drag(&mut app, MouseButton::Right, Vec2::new(12.0, -8.0));
        let after_orbit = snapshot(&app);
        assert_eq!(after_orbit.view, EditorView::Perspective);
        assert!(after_orbit.orthographic);
        assert_eq!(
            (after_orbit.center, after_orbit.extent),
            (before.center, before.extent)
        );
        drag(&mut app, MouseButton::Middle, Vec2::new(4.0, 6.0));
        let after_pan = snapshot(&app);
        assert!(after_pan.orthographic);
        assert_eq!(
            (after_pan.yaw, after_pan.pitch),
            (after_orbit.yaw, after_orbit.pitch)
        );
        assert!(after_pan.center.distance(after_orbit.center) > 1.0);
        app.world_mut().write_message(MouseWheel {
            unit: MouseScrollUnit::Line,
            x: 0.0,
            y: 1.0,
            window: Entity::PLACEHOLDER,
            phase: bevy::input::touch::TouchPhase::Moved,
        });
        app.update();
        let after_zoom = snapshot(&app);
        assert!(after_zoom.orthographic);
        assert!(after_zoom.extent < after_pan.extent);
        assert_eq!(after_zoom.center, after_pan.center);
        assert!(!app.world().resource::<MapEditorState>().dirty);
    }
}

#[test]
fn actual_camera_projection_follows_only_the_independent_switch() {
    let mut app = fixture(EditorView::Perspective);
    app.init_resource::<super::super::vegetation_ground::VegetationGroundCursor>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<SparseTerrainAuthoringState>()
        .init_resource::<AssetPreviewState>()
        .init_resource::<PlacementTransformInputState>()
        .add_systems(
            Update,
            sync_editor_viewport_camera.after(control_editor_viewport),
        );
    let camera = app
        .world_mut()
        .spawn((
            MapEditorCamera,
            Camera::default(),
            Projection::default(),
            Transform::default(),
            Hdr,
            DynamicClipmapAuthoringView {
                focus_xz: Vec2::ZERO,
                focus_y: 0.,
            },
        ))
        .id();
    app.world_mut()
        .resource_mut::<MapEditorViewportState>()
        .camera = camera;
    for orthographic in [false, true, false] {
        app.world_mut()
            .resource_mut::<MapEditorState>()
            .orthographic = orthographic;
        for view in [
            EditorView::Perspective,
            EditorView::Top,
            EditorView::Front,
            EditorView::Side,
        ] {
            app.world_mut()
                .resource_scope(|world, mut editor: Mut<MapEditorState>| {
                    let mut viewport = world.resource_mut::<MapEditorViewportState>();
                    set_editor_view(&mut editor, Some(&mut viewport), view);
                });
            app.update();
            assert_eq!(
                matches!(
                    app.world().get::<Projection>(camera),
                    Some(Projection::Orthographic(_))
                ),
                orthographic
            );
            assert_eq!(
                matches!(
                    app.world().get::<Projection>(camera),
                    Some(Projection::Perspective(_))
                ),
                !orthographic
            );
            assert!(app.world().get::<Hdr>(camera).is_some()); // 对准顶/前/侧继续使用透视，颜色格式也不因方向改变。
            assert!(
                app.world()
                    .get::<Transform>(camera)
                    .unwrap()
                    .rotation
                    .is_finite()
            );
        }
    }
}

#[test]
fn top_view_can_start_vertical_orbit_in_either_direction() {
    for delta_y in [-8.0, 8.0] {
        let mut app = fixture(EditorView::Top);
        drag(&mut app, MouseButton::Right, Vec2::new(0.0, delta_y));
        let first = snapshot(&app);
        assert_eq!(first.view, EditorView::Perspective);
        assert!((first.pitch - std::f32::consts::FRAC_PI_2) * delta_y > 0.0);
        for _ in 0..80 {
            let before = snapshot(&app);
            drag(&mut app, MouseButton::Right, Vec2::new(0.0, delta_y));
            let after = snapshot(&app);
            let before_transform =
                perspective_transform(before.center, before.extent, before.yaw, before.pitch);
            let after_transform =
                perspective_transform(after.center, after.extent, after.yaw, after.pitch);
            assert!(after_transform.rotation.is_finite());
            let alignment = before_transform
                .rotation
                .dot(after_transform.rotation)
                .abs();
            assert!(alignment > 0.999 && alignment < 0.99999); // 每帧小幅连续变化，越过顶/底极点也不能卡住或翻转。
        }
    }
}

#[test]
fn middle_drag_pans_all_views_and_keeps_the_selected_projection() {
    for view in [
        EditorView::Perspective,
        EditorView::Top,
        EditorView::Front,
        EditorView::Side,
    ] {
        let mut app = fixture(view);
        let before = snapshot(&app);
        drag(&mut app, MouseButton::Middle, Vec2::new(4.0, 6.0));
        let after = snapshot(&app);
        let delta = after.center - before.center;
        let scale = 2.0 * (PERSPECTIVE_FOV_RADIANS * 0.5).tan();
        match view {
            EditorView::Top => assert!(delta.distance(Vec3::new(-1.2, 0.0, -1.8) * scale) < 0.001),
            EditorView::Front => assert!(delta.distance(Vec3::new(-1.2, 1.8, 0.0) * scale) < 0.001),
            EditorView::Side => assert!(delta.distance(Vec3::new(0.0, 1.8, 1.2) * scale) < 0.001),
            EditorView::Perspective => assert!(delta.length() > 1.0),
        }
        assert_eq!(after.view, view);
        assert_eq!(after.orthographic, before.orthographic);
        assert_eq!(
            (after.yaw, after.pitch, after.extent),
            (before.yaw, before.pitch, before.extent)
        );
        assert!(!app.world().resource::<MapEditorState>().dirty);
    }
}

#[test]
fn wheel_zooms_all_views_without_changing_projection_or_focus() {
    for view in [
        EditorView::Perspective,
        EditorView::Top,
        EditorView::Front,
        EditorView::Side,
    ] {
        let mut app = fixture(view);
        let before = snapshot(&app);
        app.world_mut().write_message(MouseWheel {
            unit: MouseScrollUnit::Line,
            x: 0.0,
            y: 1.0,
            window: Entity::PLACEHOLDER,
            phase: bevy::input::touch::TouchPhase::Moved,
        });
        app.update();
        let after = snapshot(&app);
        assert!(after.extent < before.extent);
        assert_eq!(
            (after.view, after.center, after.yaw, after.pitch),
            (before.view, before.center, before.yaw, before.pitch)
        );
    }
}

#[test]
fn wheel_after_large_map_framing_zooms_continuously_and_remains_bounded() {
    let mut app = fixture(EditorView::Top);
    let framed_extent;
    {
        let mut viewport = app.world_mut().resource_mut::<MapEditorViewportState>();
        viewport.scene_half_extents = Vec3::new(4096.0, 256.0, 4096.0);
        viewport.render_size = UVec2::new(320, 800);
        framed_extent = super::super::camera_math::framing_extent(
            viewport.scene_half_extents,
            viewport.yaw,
            viewport.pitch,
            viewport.render_size,
            false,
        );
        assert!(framed_extent > MAX_EXTENT_METERS);
        viewport.extent = framed_extent;
    }
    for wheel in [1.0, -1.0, -100.0] {
        let before = snapshot(&app);
        app.world_mut().write_message(MouseWheel {
            unit: MouseScrollUnit::Line,
            x: 0.0,
            y: wheel,
            window: Entity::PLACEHOLDER,
            phase: bevy::input::touch::TouchPhase::Moved,
        });
        app.update();
        let after = snapshot(&app);
        if wheel == 1.0 {
            assert!((after.extent / before.extent - (-0.11_f32).exp()).abs() < 0.0001); // 首次滚轮缩放不能跳回旧20千米上限。
            assert!(after.extent > MAX_EXTENT_METERS);
        } else {
            assert!((after.extent - framed_extent).abs() < 0.01); // 超大滚轮输入也只回到有限全图范围。
        }
        assert_eq!(after.center, before.center);
        assert!(!app.world().resource::<MapEditorState>().dirty);
    }
}

#[test]
fn blocked_navigation_consumes_motion_without_replaying_it_afterwards() {
    for reason in 0..4 {
        let mut app = fixture(EditorView::Top);
        let before = snapshot(&app);
        {
            let mut editor = app.world_mut().resource_mut::<MapEditorState>();
            match reason {
                0 => editor.open = false,
                1 => editor.new_map_dialog_open = true,
                2 => editor.map_transition_in_progress = true,
                _ => {
                    editor.pending_map_transition =
                        Some(feathrise_map_content::MapId("MAP-waiting".to_owned()))
                }
            }
        }
        drag(&mut app, MouseButton::Right, Vec2::new(12.0, 8.0));
        assert_eq!(snapshot(&app), before);
        {
            let mut editor = app.world_mut().resource_mut::<MapEditorState>();
            editor.open = true;
            editor.new_map_dialog_open = false;
            editor.map_transition_in_progress = false;
            editor.pending_map_transition = None;
        }
        app.update(); // 仍持续按右键；被挡住的旧Motion不能补做一次旋转或切换投影。
        assert_eq!(snapshot(&app), before);
    }
}
