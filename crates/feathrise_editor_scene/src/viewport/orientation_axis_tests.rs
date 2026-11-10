// XYZ方向轴世界到屏幕投影的纯算法回归测试。
use super::*;

#[test]
fn identity_camera_projects_x_right_y_up_and_z_into_the_screen_center() {
    let x = project_world_axis(Quat::IDENTITY, Vec3::X);
    let y = project_world_axis(Quat::IDENTITY, Vec3::Y);
    let z = project_world_axis(Quat::IDENTITY, Vec3::Z);
    assert!(x.offset.x > 24.9 && x.offset.y.abs() < 0.001); // 相机局部X对应UI向右。
    assert!(y.offset.y < -24.9 && y.offset.x.abs() < 0.001); // 相机局部Y对应UI向上。
    assert!(z.offset.length() < 0.001 && z.depth > 0.999); // 朝观察者的轴收缩到中心并保留深度。
}

#[test]
fn top_camera_widget_matches_world_x_right_negative_z_up_and_y_facing_viewer() {
    let camera = Transform::IDENTITY.looking_to(Vec3::NEG_Y, Vec3::NEG_Z);
    let x = project_world_axis(camera.rotation, Vec3::X);
    let y = project_world_axis(camera.rotation, Vec3::Y);
    let z = project_world_axis(camera.rotation, Vec3::Z);
    assert!(x.offset.x > 24.9 && x.offset.y.abs() < 0.001); // 世界X仍在屏幕右侧。
    assert!(z.offset.y > 24.9 && z.offset.x.abs() < 0.001); // 顶视屏幕上方是世界-Z，因此+Z显示在下方。
    assert!(y.offset.length() < 0.001 && y.depth > 0.999); // 世界+Y指向顶视观察者并显示在控件中心。
}

#[test]
fn sync_system_accepts_disjoint_mutable_line_and_endpoint_queries() {
    let mut app = App::new(); // 直接初始化正式系统参数，旧实现会在首帧因两个可变UI Query重叠而触发B0001。
    app.init_resource::<SceneEditorState>(); // 同步系统只依赖编辑器会话资源和三个实体集合。
    app.world_mut().resource_mut::<SceneEditorState>().open = true; // 打开状态确保首帧真正执行方向轴同步。
    app.add_systems(Update, sync_editor_orientation_axis); // 使用正式系统而不是只测试投影纯函数。
    app.world_mut().spawn((
        SceneEditorCamera,      // 唯一编辑相机供系统读取旋转。
        Transform::default(), // 单位旋转让X轴线拥有非零屏幕长度。
    ));
    let line = app
        .world_mut()
        .spawn((
            SceneEditorOrientationAxisLine {
                world_axis: Vec3::X, // 测试轴线使用世界X正方向。
            },
            Node::default(),        // 正式轴线查询需要可变UI布局组件。
            UiTransform::default(), // 正式轴线查询需要可变UI旋转组件。
            Visibility::Hidden,     // 首帧同步后应恢复显示。
        ))
        .id(); // 保存实体以确认系统确实完成更新而不是提前退出。
    let endpoint = app
        .world_mut()
        .spawn((
            SceneEditorOrientationAxisEndpoint {
                world_axis: Vec3::X, // 测试端点与轴线属于同一世界轴。
                positive: true,      // 正端点在单位相机下应保持可见。
            },
            Node::default(),    // 正式端点查询需要可变UI布局组件。
            Visibility::Hidden, // 首帧同步后应恢复显示。
            ZIndex(0),          // 正式端点查询会按相机空间深度更新层级。
        ))
        .id(); // 保存实体以核对端点查询也正常执行。

    app.update(); // 初始化并运行系统；最重要的断言是本行不再触发Bevy B0001 panic。

    assert_eq!(
        app.world().get::<Visibility>(line),
        Some(&Visibility::Inherited)
    ); // 轴线查询在互斥过滤后仍应正常写入可见状态。
    assert_eq!(
        app.world().get::<Visibility>(endpoint),
        Some(&Visibility::Inherited)
    ); // 端点查询同样不能因修复借用冲突而停止工作。
}

#[test]
fn six_axis_clicks_preserve_focus_and_center_toggles_projection_without_changing_angles() {
    let mut app = App::new();
    app.insert_resource(SceneEditorState {
        open: true,
        ..default()
    })
    .init_resource::<SceneEditorNumericScrubMouseGuard>()
    .insert_resource(SceneEditorViewportState {
        _image: Handle::default(),
        camera: Entity::PLACEHOLDER,
        center: Vec3::new(4.0, 8.0, -3.0),
        scene_center: Vec3::ZERO,
        scene_half_extents: Vec3::splat(10.0),
        extent: 35.0,
        yaw: 0.2,
        pitch: 0.4,
        framed_scene_id: None,
        render_size: UVec2::new(640, 360),
    })
    .add_systems(
        Update,
        (
            (|mut editor: ResMut<SceneEditorState>| editor.suppress_world_pick_this_frame = false),
            orientation_axis_input,
        )
            .chain(),
    );
    let center = app.world().resource::<SceneEditorViewportState>().center;
    for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
        for positive in [true, false] {
            let button = app
                .world_mut()
                .spawn((
                    SceneEditorOrientationAxisEndpoint {
                        world_axis: axis,
                        positive,
                    },
                    EditorButtonClick::default(),
                    Visibility::Inherited,
                    Node::default(),
                ))
                .id();
            app.update();
            assert_eq!(
                app.world().resource::<SceneEditorViewportState>().center,
                center
            );
            app.world_mut()
                .entity_mut(button)
                .insert(EditorButtonClick::triggered());
            app.update();
            let viewport = app.world().resource::<SceneEditorViewportState>();
            let view = super::super::camera_math::perspective_transform(
                Vec3::ZERO,
                viewport.extent,
                viewport.yaw,
                viewport.pitch,
            );
            assert!(
                (view.rotation * Vec3::Z - axis * if positive { 1.0 } else { -1.0 }).length()
                    < 0.0001
            );
            assert_eq!(viewport.center, center);
            assert_eq!(viewport.extent, 35.0);
            assert!(app.world().resource::<SceneEditorState>().orthographic);
            assert!(
                app.world()
                    .resource::<SceneEditorState>()
                    .suppress_world_pick_this_frame
            );
        }
    }
    let aligned = app
        .world_mut()
        .spawn((
            SceneEditorOrientationAxisEndpoint {
                world_axis: Vec3::Z,
                positive: false,
            },
            EditorButtonClick::triggered(),
            Visibility::Inherited,
            Node::default(),
        ))
        .id();
    app.update();
    assert!(
        app.world().resource::<SceneEditorViewportState>().yaw.abs() < 0.001,
        "再次点已对准轴翻到反方向"
    );
    app.world_mut().entity_mut(aligned).despawn();
    let before = {
        let v = app.world().resource::<SceneEditorViewportState>();
        (v.yaw, v.pitch)
    };
    let toggle = app
        .world_mut()
        .spawn((
            OrientationProjectionToggle,
            EditorButtonClick::triggered(),
            Node::default(),
        ))
        .id();
    app.update();
    assert!(!app.world().resource::<SceneEditorState>().orthographic);
    assert_eq!(
        {
            let v = app.world().resource::<SceneEditorViewportState>();
            (v.yaw, v.pitch)
        },
        before
    );
    app.world_mut()
        .resource_mut::<SceneEditorNumericScrubMouseGuard>()
        .advance(true, true, false);
    app.world_mut()
        .entity_mut(toggle)
        .insert(EditorButtonClick::triggered());
    app.update();
    assert!(!app.world().resource::<SceneEditorState>().orthographic);
    app.world_mut()
        .resource_mut::<SceneEditorNumericScrubMouseGuard>()
        .advance(false, false, false);
    app.update();
    assert!(
        !app.world().resource::<SceneEditorState>().orthographic,
        "静默门归还后不得重放被拒收点击"
    );
}
