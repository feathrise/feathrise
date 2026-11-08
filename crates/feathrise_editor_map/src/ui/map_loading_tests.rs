//! 编辑器切图的真实进度、失败反馈与鼠标/键盘门回归；不加载或写入地图资产。

use feathrise_editor_ui::InteractionButton;
use super::*;
use feathrise_scene_core::state::PendingSceneTransition;

#[test]
fn loading_footer_stays_one_line_with_long_names_and_failure_details() {
    use bevy::app::{HierarchyPropagatePlugin, PropagateSet};
    use bevy::camera::{ComputedCameraValues, RenderTargetInfo, Viewport};
    use bevy::ui::{
        ComputedUiRenderTargetInfo, ComputedUiTargetCamera, ui_layout_system,
        ui_surface::UiSurface, update::propagate_ui_target_cameras,
    };
    for (width, scale, dpi) in [(360.0, 1.5, 1.25), (800.0, 1.0, 1.0), (1200.0, 0.6, 1.0)] {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            bevy::scene::ScenePlugin, // BSN真实实例化需要ScenePatch资产容器，与正式DefaultPlugins一致。
            bevy::text::TextPlugin,
            bevy::transform::TransformPlugin,
        ));
        app.add_plugins(HierarchyPropagatePlugin::<ComputedUiTargetCamera>::new(
            PostUpdate,
        ));
        app.add_plugins(HierarchyPropagatePlugin::<ComputedUiRenderTargetInfo>::new(
            PostUpdate,
        ));
        app.init_resource::<UiSurface>()
            .insert_resource(UiScale(scale));
        app.configure_sets(
            PostUpdate,
            PropagateSet::<ComputedUiTargetCamera>::default()
                .after(propagate_ui_target_cameras)
                .before(bevy::ui::widget::measure_text_system),
        );
        app.configure_sets(
            PostUpdate,
            PropagateSet::<ComputedUiRenderTargetInfo>::default()
                .after(propagate_ui_target_cameras)
                .before(bevy::ui::widget::measure_text_system),
        );
        app.add_systems(
            PostUpdate,
            (
                propagate_ui_target_cameras,
                bevy::ui::widget::measure_text_system.after(bevy::text::detect_text_needs_rerender),
                ui_layout_system,
            )
                .chain(),
        );
        let face = app
            .world_mut()
            .resource_mut::<Assets<Font>>()
            .add(Font::from_bytes(
                feathrise_runtime::app::default_font::DEFAULT_FONT_BYTES.to_vec(),
            ));
        let physical_size = UVec2::new((width * scale * dpi) as u32, (24.0 * scale * dpi) as u32);
        app.world_mut().spawn((
            Camera2d,
            Camera {
                computed: ComputedCameraValues {
                    target_info: Some(RenderTargetInfo {
                        physical_size,
                        scale_factor: dpi,
                    }),
                    ..default()
                },
                viewport: Some(Viewport {
                    physical_size,
                    ..default()
                }),
                ..default()
            },
        ));
        let host = app
            .world_mut()
            .spawn(Node {
                width: Val::Px(width),
                height: Val::Px(24.0),
                align_items: AlignItems::Center,
                ..default()
            })
            .id();
        let panel = app
            .world_mut()
            .spawn_scene(map_loading_panel())
            .unwrap()
            .id();
        app.world_mut().entity_mut(host).add_child(panel);
        assert!(app.world().get::<GlobalZIndex>(panel).is_none(), "加载行继承底栏层级，不再创建中央覆盖层");
        for mut font in app
            .world_mut()
            .query::<&mut TextFont>()
            .iter_mut(app.world_mut())
        {
            font.font = face.clone().into(); // 使用项目真实中文字体，不能靠默认拉丁字体的空白测量判断不溢出。
        }
        let transition = PendingSceneTransition::editor_map(
            ScenePhase::Active,
            "用于窄窗口检查的中文地图名称".repeat(30),
        );
        let mut progress = LoadingProgress::for_transition(&transition);
        progress.stage = LoadingStage::Failed;
        progress.label = format!(
            "地图资源不可用：{}",
            "assets/maps/很长的中文地图目录/terrain.glb/".repeat(4)
        );
        app.insert_resource(MapEditorState {
            open: true,
            map_transition_in_progress: true,
            ..default()
        })
        .insert_resource(progress)
        .add_systems(Update, sync_map_loading);
        for _ in 0..4 {
            app.update();
        }
        let panel_size = app.world().get::<ComputedNode>(panel).unwrap().size();
        let host_size = app.world().get::<ComputedNode>(host).unwrap().size();
        assert!(panel_size.x > 0.0 && panel_size.y > 0.0);
        assert!(
            panel_size.x <= host_size.x + 1.0 && panel_size.y <= host_size.y + 1.0,
            "长地图名和失败原因不能撑高24px底栏：{panel_size:?} / {host_size:?}"
        );
        let world = app.world_mut();
        let (detail, detail_node, source) = world
            .query_filtered::<(&Text, &Node, &PanelStatusText), With<MapEditorMapLoadingDetail>>()
            .single(world).unwrap();
        assert_eq!(detail_node.display, Display::None, "完整诊断只能进入原详情窗口");
        assert_eq!(source.0, "地图加载");
        assert!(detail.contains("用于窄窗口检查的中文地图名称"));
        assert!(detail.contains("assets/maps/很长的中文地图目录/terrain.glb/"));
        assert!(detail.contains("取消会保留草稿与其他标签"));
        let (title_parent, title_layout) = world
            .query_filtered::<(&ChildOf, &ComputedNode), With<MapEditorMapLoadingTitle>>()
            .single(world).unwrap();
        assert_eq!(world.get::<Node>(title_parent.parent()).unwrap().overflow, Overflow::clip_x());
        let clip_layout = world.get::<ComputedNode>(title_parent.parent()).unwrap();
        assert!(title_layout.size().x > clip_layout.size().x, "长标题应由独立父节点裁切，不换行挤高底栏或盖住恢复按钮");
        for (button, computed) in world.query_filtered::<(&Node, &ComputedNode), With<MapLoadingRecoveryButton>>().iter(world) {
            assert_eq!(button.height, px(22));
            assert_eq!(button.flex_shrink, 0.0, "取消与重试始终保留完整命中区");
            assert!(computed.size().min_element() > 0.0 && computed.size().y <= panel_size.y + 1.0);
        }
        for node in app
            .world_mut()
            .query_filtered::<&ComputedNode, (With<Text>, Without<MapEditorMapLoadingDetail>)>()
            .iter(app.world())
        {
            assert!(node.size().y > 0.0, "中文文本必须实际参与排版");
            assert!(
                node.content_size().y <= node.size().y + 2.0 && node.size().y <= panel_size.y + 1.0,
                "底栏可见文字始终保持单行；长内容由裁切及详情承接"
            );
        }
    }
}

#[test]
fn loading_workbench_shows_progress_and_releases_input_only_when_session_unlocks() {
    let mut app = App::new();
    app.insert_resource(MapEditorState {
        open: true,
        map_transition_in_progress: true,
        ..default()
    })
    .insert_resource(LoadingProgress::for_transition(
        &PendingSceneTransition::editor_map(ScenePhase::Active, "测试地图"),
    ))
    .init_resource::<ButtonInput<KeyCode>>()
    .add_systems(
        Update,
        (suppress_map_loading_shortcuts, sync_map_loading).chain(),
    );
    let panel = app
        .world_mut()
        .spawn((MapEditorMapLoadingPanel, Node::default()))
        .id();
    let blocker = app
        .world_mut()
        .spawn((
            MapEditorMapLoadingInputBlocker,
            Node::default(),
            Interaction::Pressed,
            FocusPolicy::Pass,
        ))
        .id();
    let fill = app
        .world_mut()
        .spawn((MapEditorMapLoadingFill, Node::default()))
        .id();
    let title = app
        .world_mut()
        .spawn((
            MapEditorMapLoadingTitle,
            Text::default(),
            TextColor::default(),
        ))
        .id();
    let detail = app
        .world_mut()
        .spawn((
            MapEditorMapLoadingDetail,
            Text::default(),
            TextColor::default(),
        ))
        .id();
    let percent = app
        .world_mut()
        .spawn((
            MapEditorMapLoadingPercent,
            Text::default(),
            TextColor::default(),
        ))
        .id();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::KeyS);
    {
        let mut progress = app.world_mut().resource_mut::<LoadingProgress>();
        progress.stage = LoadingStage::Collision;
        progress.completed = 1;
        progress.total = 1;
        progress.label = "地图碰撞已就绪".to_owned();
    }
    app.update();
    assert!(app.world().resource::<MapEditorState>().open);
    assert!(
        !app.world()
            .resource::<ButtonInput<KeyCode>>()
            .just_pressed(KeyCode::KeyS)
    );
    assert_eq!(
        app.world().get::<Text>(title).unwrap().as_str(),
        "正在打开“测试地图”"
    );
    let full_detail = app.world().get::<Text>(detail).unwrap().as_str();
    assert!(full_detail.starts_with("正在打开“测试地图”\n地图碰撞已就绪\n82% · 已耗时 "));
    assert!(full_detail.contains("取消会保留草稿与其他标签"));
    assert!(app.world().get::<Text>(percent).unwrap().starts_with("82% · "));
    assert_eq!(
        app.world().get::<Node>(fill).unwrap().width,
        Val::Percent(82.0)
    );
    assert_eq!(
        *app.world().get::<FocusPolicy>(blocker).unwrap(),
        FocusPolicy::Block
    );
    {
        let mut progress = app.world_mut().resource_mut::<LoadingProgress>();
        progress.stage = LoadingStage::Failed;
        progress.label = "地形资源加载失败".to_owned();
    }
    app.update();
    assert_eq!(
        app.world().get::<Text>(title).unwrap().as_str(),
        "无法打开“测试地图”"
    );
    assert!(app.world().get::<Text>(detail).unwrap().starts_with("无法打开“测试地图”\n地形资源加载失败\n"));
    assert!(app.world().get::<Text>(percent).unwrap().starts_with("失败 · "));
    app.world_mut().remove_resource::<LoadingProgress>();
    app.update();
    assert_eq!(
        app.world().get::<Node>(panel).unwrap().display,
        Display::Flex
    ); // 进度移除尚未代表编辑会话完成，等待正式就绪收口。
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .map_transition_in_progress = false;
    app.update();
    assert_eq!(
        app.world().get::<Node>(panel).unwrap().display,
        Display::None
    );
    assert_eq!(
        app.world().get::<Node>(blocker).unwrap().display,
        Display::None
    );
    assert_eq!(
        *app.world().get::<FocusPolicy>(blocker).unwrap(),
        FocusPolicy::Pass
    );
    assert_eq!(
        *app.world().get::<Interaction>(blocker).unwrap(),
        Interaction::None
    );
    assert!(app.world().get::<Text>(detail).unwrap().is_empty(), "加载结束后详情不能残留旧失败原因");
}

#[test]
fn loading_input_gate_blocks_real_ui_focus_without_being_a_button() {
    use bevy::app::HierarchyPropagatePlugin;
    use bevy::camera::{ComputedCameraValues, RenderTargetInfo};
    use bevy::ecs::system::RunSystemOnce;
    use bevy::input::touch::Touches;
    use bevy::ui::{ComputedUiTargetCamera, UiStack, ui_focus_system, update::propagate_ui_target_cameras};
    use bevy::window::PrimaryWindow;

    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default(), bevy::scene::ScenePlugin))
        .add_plugins(HierarchyPropagatePlugin::<ComputedUiTargetCamera>::new(PostUpdate))
        .init_resource::<UiScale>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<Touches>()
        .insert_resource(MapEditorState { open: true, map_transition_in_progress: true, ..default() })
        .add_systems(Update, (sync_map_loading, propagate_ui_target_cameras).chain());
    let mut window = Window::default();
    window.resolution.set_physical_resolution(640, 360);
    window.set_cursor_position(Some(Vec2::new(100.0, 100.0)));
    app.world_mut().spawn((window, PrimaryWindow));
    let camera = app.world_mut().spawn((Camera2d, Camera {
        computed: ComputedCameraValues {
            target_info: Some(RenderTargetInfo { physical_size: UVec2::new(640, 360), scale_factor: 1.0 }),
            ..default()
        },
        ..default()
    })).id();
    let behind = app.world_mut().spawn((
        InteractionButton, Node::default(), FocusPolicy::Block, UiTargetCamera(camera),
        ComputedNode { size: Vec2::new(640.0, 332.0), ..default() },
        UiGlobalTransform::from_xy(320.0, 194.0), InheritedVisibility::VISIBLE,
    )).id();
    let blocker = app.world_mut().spawn_scene(map_loading_input_blocker()).unwrap().id();
    app.world_mut().entity_mut(blocker).insert((
        UiTargetCamera(camera), ComputedNode { size: Vec2::new(640.0, 332.0), ..default() },
        UiGlobalTransform::from_xy(320.0, 194.0), InheritedVisibility::VISIBLE,
    ));
    // 使用正式命中系统和真实上下遮挡顺序，不能靠手工把底层Interaction清空证明安全。
    app.insert_resource(UiStack { partition: vec![0..2], uinodes: vec![behind, blocker] });
    app.update();
    assert!(app.world().get::<InteractionButton>(blocker).is_none(), "纯输入门不能参与按钮悬停上色");
    assert_eq!(app.world().get::<BackgroundColor>(blocker).unwrap().0, Color::NONE);
    assert_eq!(app.world().get::<Node>(blocker).unwrap().top, px(28));
    app.world_mut().run_system_once(ui_focus_system).unwrap();
    assert_eq!(*app.world().get::<Interaction>(blocker).unwrap(), Interaction::Hovered);
    assert_eq!(*app.world().get::<Interaction>(behind).unwrap(), Interaction::None);
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
    app.world_mut().run_system_once(ui_focus_system).unwrap();
    assert_eq!(*app.world().get::<Interaction>(blocker).unwrap(), Interaction::Pressed);
    assert_eq!(*app.world().get::<Interaction>(behind).unwrap(), Interaction::None, "加载中的作者按钮不能被按压");
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
    app.world_mut().run_system_once(ui_focus_system).unwrap();
    assert_eq!(*app.world().get::<Interaction>(behind).unwrap(), Interaction::None, "原地释放同样不能点透");
    assert_eq!(app.world().get::<BackgroundColor>(blocker).unwrap().0, Color::NONE);
}
