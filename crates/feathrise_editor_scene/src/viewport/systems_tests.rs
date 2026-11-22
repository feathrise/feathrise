// 编辑器视口导航与射线命中的纯算法回归测试。
use super::*;

#[test]
fn resident_viewports_keep_hidden_targets_and_match_first_visible_frame() {
    let mut app = App::new();
    app.init_resource::<bevy::picking::pointer::PointerMap>()
        .init_resource::<Assets<Image>>()
        .add_systems(PostUpdate, (
            bind_editor_viewport_camera,
            bevy::ui::widget::update_viewport_render_target_size,
            sync_editor_viewport_render_size,
        ).chain());
    let world = app.world_mut();
    let start = UVec2::new(640, 360);
    let node = world.spawn((SceneEditorViewport, ViewportNode { camera: None },
        ComputedNode { size: start.as_vec2(), ..default() })).id();
    let pointer = *world.get::<bevy::picking::pointer::PointerId>(node).unwrap();
    let mut spawn = |size: UVec2| {
        let mut image = Image { data: None, ..default() };
        image.resize(Extent3d { width: size.x, height: size.y, depth_or_array_layers: 1 });
        let handle = world.resource_mut::<Assets<Image>>().add(image);
        let camera = world.spawn((SceneEditorCamera, Camera::default(), RenderTarget::from(handle.clone()),
            Projection::Perspective(PerspectiveProjection::default()))).id();
        SceneEditorViewportState { _image: handle, camera, center: Vec3::ZERO,
            scene_center: Vec3::ZERO, scene_half_extents: Vec3::ONE, extent: 10.0,
            yaw: 0.0, pitch: 0.5, framed_scene_id: None, render_size: size }
    };
    let first = spawn(start);
    let second = spawn(UVec2::new(800, 450));
    let first_camera = first.camera;
    let first_target = first._image.clone();
    let second_camera = second.camera;
    world.insert_resource(first);

    app.update();
    let assert_visible = |world: &World, expected_camera: Entity, size: UVec2| {
        let viewport = world.resource::<SceneEditorViewportState>();
        assert_eq!(world.get::<ViewportNode>(node).unwrap().camera, Some(expected_camera));
        assert_eq!(world.get::<bevy::picking::pointer::PointerId>(node), Some(&pointer));
        assert_eq!(world.get::<Camera>(expected_camera).unwrap().physical_target_size(), Some(size));
        assert_eq!(world.resource::<Assets<Image>>().get(&viewport._image).unwrap().size(), size);
        assert_eq!(viewport.render_size, size);
        let Projection::Perspective(projection) = world.get::<Projection>(expected_camera).unwrap() else { panic!("测试相机应使用透视"); };
        assert_eq!(projection.aspect_ratio, size.x as f32 / size.y as f32);
    };
    assert_visible(app.world(), first_camera, start);

    // 开始页隐藏实际布局，即使相机仍处于活动状态，也不能缩小目标纹理。
    app.world_mut().get_mut::<ComputedNode>(node).unwrap().size = Vec2::ZERO;
    app.update();
    assert_eq!(app.world().get::<ViewportNode>(node).unwrap().camera, None);
    assert_eq!(app.world().resource::<Assets<Image>>().get(&first_target).unwrap().size(), start);
    let resumed = UVec2::new(960, 500);
    app.world_mut().get_mut::<ComputedNode>(node).unwrap().size = resumed.as_vec2();
    app.update();
    assert_visible(app.world(), first_camera, resumed);

    // 暖场景以同一节点接回不同相机，且相机使用布局调整后的同帧宽高比。
    app.world_mut().get_mut::<Camera>(first_camera).unwrap().is_active = false;
    app.world_mut().insert_resource(second);
    app.update();
    assert_visible(app.world(), second_camera, resumed);
    assert_eq!(app.world().resource::<Assets<Image>>().get(&first_target).unwrap().size(), resumed);
}
const APPLICATION_MODEL_LAYER: usize = 24; // 中性应用夹具，不依赖任何游戏角色层定义。
const APPLICATION_TRANSIENT_LAYER: usize = 25;

#[test]
fn editor_camera_requires_explicit_application_layers_without_adding_transient_views() {
    let layers = editor_viewport_render_layers(None);
    assert!(!layers.intersects(&RenderLayers::layer(APPLICATION_MODEL_LAYER)));
    assert_eq!(layers, editor_viewport_render_layers(Some(&EditorApplicationViewLayers::default())));

    let application = EditorApplicationViewLayers {
        camera: RenderLayers::layer(APPLICATION_MODEL_LAYER),
        framing: RenderLayers::layer(APPLICATION_TRANSIENT_LAYER),
    };
    let layers = editor_viewport_render_layers(Some(&application));
    assert!(layers.intersects(&RenderLayers::layer(APPLICATION_MODEL_LAYER)));
    assert!(layers.intersects(&RenderLayers::layer(0))); // 应用输入只能追加，场景与环境原集合仍在。
    assert!(layers.intersects(&RenderLayers::layer(WEATHER_EFFECT_LAYER)));
    assert!(!layers.intersects(&RenderLayers::layer(APPLICATION_TRANSIENT_LAYER))); // 取景资格不会隐式变为相机可见资格。
}

#[test]
fn f1_air_wall_layer_does_not_enter_f12_authoring_camera() {
    let layers = editor_viewport_render_layers(None); // 验证实际相机配置，不复制层集合。
    assert!(!layers.intersects(&RenderLayers::layer(AIR_WALL_GAMEPLAY_LAYER)));
    assert!(!layers.intersects(&RenderLayers::layer(AIR_WALL_OUTLINE_GAMEPLAY_LAYER)));
    assert!(layers.intersects(&RenderLayers::layer(SCENE_EDITOR_BRUSH_RENDER_LAYER))); // F12空气墙辅助线继续独立可见。
    let application = EditorApplicationViewLayers {
        camera: RenderLayers::from_layers(&[AIR_WALL_GAMEPLAY_LAYER, AIR_WALL_OUTLINE_GAMEPLAY_LAYER]),
        ..default()
    };
    assert_eq!(layers, editor_viewport_render_layers(Some(&application))); // 应用追加不能突破作者/运行调试轮廓的隔离。
}

#[test]
fn right_drag_vertical_motion_uses_direct_pitch_direction() {
    let pitch = 0.64; // 使用默认透视俯仰角验证两个拖动方向。
    assert!(orbit_pitch_after_drag(pitch, 10.0) > pitch); // 鼠标向下拖时俯仰角应增加。
    assert!(orbit_pitch_after_drag(pitch, -10.0) < pitch); // 鼠标向上拖时俯仰角应减小。
}

#[test]
fn right_drag_can_orbit_below_terrain_and_continuously_cross_camera_poles() {
    let underside_pitch = orbit_pitch_after_drag(0.64, -200.0); // 持续向上拖必须越过水平线进入地形下方。
    assert!(underside_pitch < 0.0); // 负俯仰是相机位于轨道中心下方的明确身份。
    let top = std::f32::consts::FRAC_PI_2;
    assert!(orbit_pitch_after_drag(top, 10.0) > top); // 顶视向下拖也能继续旋转，不能停在原极点保护夹角。
    assert!(orbit_pitch_after_drag(top, -10.0) < top);
    for motion in [-10_000.0, 10_000.0] {
        let pitch = orbit_pitch_after_drag(0.64, motion);
        assert!(pitch.is_finite() && pitch.abs() <= std::f32::consts::PI); // 完整转圈后角度有界，姿态仍连续。
    }
}

#[test]
fn changing_projection_preserves_focus_plane_screen_scale_at_every_direction() {
    let center = Vec3::new(10.0, 20.0, 30.0);
    for view in [EditorView::Perspective, EditorView::Top, EditorView::Front, EditorView::Side] {
        let (yaw, pitch) = orbit_angles_from_view(view);
        for extent in [MIN_EXTENT_METERS, 120.0, MAX_EXTENT_METERS] {
            let viewport = SceneEditorViewportState {
                _image: Handle::default(), camera: Entity::PLACEHOLDER,
                center, scene_center: Vec3::ZERO, scene_half_extents: Vec3::new(500.0, 80.0, 300.0),
                extent, yaw, pitch, framed_scene_id: None, render_size: UVec2::new(800, 600),
            };
            let (mut perspective, perspective_pose) = editor_camera_pose(false, &viewport);
            let (mut orthographic, orthographic_pose) = editor_camera_pose(true, &viewport);
            perspective.update(800.0, 600.0);
            orthographic.update(800.0, 600.0);
            assert!(perspective_pose.rotation.dot(orthographic_pose.rotation).abs() > 0.99999);
            let point = center + perspective_pose.right() * (extent * 0.1)
                + perspective_pose.up() * (extent * 0.05); // 在焦点平面取偏离中心的点，不能只检查必为屏幕中心的焦点。
            let to_screen = |projection: &Projection, transform: Transform| {
                let view_point = transform.rotation.inverse() * (point - transform.translation);
                let clip = projection.get_clip_from_view() * view_point.extend(1.0);
                Vec2::new(clip.x, clip.y) / clip.w
            };
            assert!(to_screen(&perspective, perspective_pose)
                .distance(to_screen(&orthographic, orthographic_pose)) < 0.0001); // 包括缩放上下限，来回切换不能累积缩放误差。
        }
    }
}

// 实际GLB三角形射线命中由共享TerrainEditingIndex测试覆盖；视口不再拥有第二套规则高度场算法。
