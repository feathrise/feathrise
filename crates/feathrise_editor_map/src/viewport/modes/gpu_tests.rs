//! 真实GPU离屏检查竖向工具栏的图标、鼠标命中和旁侧菜单；不启动窗口或保存作者设置。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::InteractionButton;
use super::*;
use bevy::ecs::system::RunSystemOnce;
use bevy::input::{ButtonState, mouse::MouseButtonInput};
use bevy::render::{
    gpu_readback::{Readback, ReadbackComplete},
    render_resource::{TextureFormat, TextureUsages},
};
use bevy::window::{ExitCondition, PrimaryWindow};
use bevy::winit::WinitPlugin;

#[derive(Resource)]
struct TestPointer {
    window: Entity,
    camera: Entity,
}

/// 只在正式命中时读取测试窗口光标，仍向原离屏图像渲染，不创建原生窗口。
fn official_pointer(world: &mut World) {
    let pointer = world.resource::<TestPointer>();
    let (window, camera) = (pointer.window, pointer.camera);
    let target = world
        .get::<bevy::camera::RenderTarget>(camera)
        .unwrap()
        .clone();
    *world.get_mut::<bevy::camera::RenderTarget>(camera).unwrap() =
        bevy::camera::RenderTarget::Window(bevy::window::WindowRef::Entity(window));
    world.run_system_once(bevy::ui::ui_focus_system).unwrap();
    *world.get_mut::<bevy::camera::RenderTarget>(camera).unwrap() = target;
}

fn bounds(app: &App, entity: Entity) -> Rect {
    Rect::from_center_size(
        app.world()
            .get::<UiGlobalTransform>(entity)
            .unwrap()
            .translation,
        app.world().get::<ComputedNode>(entity).unwrap().size(),
    )
}

fn click(app: &mut App, target: Entity) {
    let position = bounds(app, target).center();
    let window = app.world().resource::<TestPointer>().window;
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .set_physical_cursor_position(Some(position.as_dvec2()));
    app.world_mut().write_message(MouseButtonInput {
        button: MouseButton::Left,
        state: ButtonState::Pressed,
        window,
    });
    app.update();
    assert_eq!(
        app.world().get::<UiInteraction>(target),
        Some(&UiInteraction::Pressed)
    );
    assert!(
        app.world()
            .resource::<ViewportModeState>()
            .pointer_over_controls,
        "图标与菜单按钮的正式命中必须在同帧阻断世界操作"
    );
    app.world_mut().write_message(MouseButtonInput {
        button: MouseButton::Left,
        state: ButtonState::Released,
        window,
    });
    app.update();
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .set_physical_cursor_position(Some(Vec2::new(620.0, 340.0).as_dvec2()));
    app.update();
}

fn frames(app: &mut App, count: usize) {
    for _ in 0..count {
        app.update();
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
}

fn snapshot(app: &mut App, target: Handle<Image>, name: &str) {
    let (sender, receiver) = std::sync::mpsc::channel();
    app.world_mut().spawn(Readback::texture(target)).observe(
        move |event: On<ReadbackComplete>, mut commands: Commands| {
            let _ = sender.send(event.data.clone());
            commands.entity(event.entity).try_despawn();
        },
    );
    let started = std::time::Instant::now();
    let mut bytes = loop {
        app.update();
        if let Ok(bytes) = receiver.try_recv() {
            break bytes;
        }
        assert!(started.elapsed().as_secs() < 10, "模式浮层GPU回读超时");
        std::thread::sleep(std::time::Duration::from_millis(16));
    };
    // 640像素宽的BGRA行满足256字节对齐，回读不需要额外剥离行填充。
    assert_eq!(bytes.len(), 640 * 360 * 4);
    for pixel in bytes.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    assert!(
        bytes
            .chunks_exact(4)
            .filter(|pixel| pixel[0] > 100 && pixel[1] > 100 && pixel[2] > 100)
            .count()
            > 100,
        "实际工具图标和菜单中文必须绘制"
    );
    let world = app.world_mut();
    assert_eq!(world.query::<&ImageNode>().iter(world).count(), 5);
    for (image, node, transform) in world
        .query::<(&ImageNode, &ComputedNode, &UiGlobalTransform)>()
        .iter(world)
    {
        assert!(
            world
                .resource::<Assets<Image>>()
                .get(&image.image)
                .is_some(),
            "正式图标资产必须完成加载"
        );
        if node.size().min_element() <= 0.0 {
            continue;
        } // 点边面只收起普通画笔图标，不要求隐藏节点产生像素。
        let rect = Rect::from_center_size(transform.translation, node.size());
        let mut ink = 0;
        for y in (rect.min.y.ceil() as usize)..(rect.max.y.floor() as usize) {
            for x in (rect.min.x.ceil() as usize)..(rect.max.x.floor() as usize) {
                let pixel = &bytes[(y * 640 + x) * 4..][..3];
                if pixel.iter().all(|channel| *channel > 150) {
                    ink += 1;
                }
            }
        }
        assert!(ink > 12, "透明线框图标应有实际像素，不能只存在空按钮矩形");
    }
    let output =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.local/validation");
    std::fs::create_dir_all(&output).unwrap();
    image::RgbaImage::from_raw(640, 360, bytes)
        .unwrap()
        .save(output.join(name))
        .unwrap();
}

#[test]
#[ignore = "需要真实GPU；核对170%缩放的竖向图标、正式鼠标命中与菜单像素，不启动原生窗口"]
fn viewport_mode_rail_gpu_actual_clicks_and_context_snapshot() {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                ..default()
            })
            .set(AssetPlugin {
                file_path: concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets").into(),
                watch_for_changes_override: Some(false),
                ..default()
            })
            .disable::<WinitPlugin>()
            .disable::<bevy::log::LogPlugin>(),
    )
    .add_plugins(feathrise_editor_ui::EditorThemePlugin)
    .add_plugins(feathrise_editor_ui::EditorButtonInputPlugin)
    .insert_resource(UiScale(1.7))
    .insert_resource(ClearColor(Color::srgb(0.46, 0.35, 0.23))) // 测试底色用于辨认背景边界，工具配色仍走正式主题。
    .init_resource::<ViewportModeState>()
    .insert_resource(MapEditorState {
        open: true,
        ..default()
    })
    .init_resource::<MapEditorNumericScrubMouseGuard>()
    .add_systems(
        PreUpdate,
        official_pointer
            .after(bevy::ui::UiSystems::Focus)
            .before(feathrise_editor_ui::update_editor_button_clicks), // 离屏正式命中先于释放点击捕获，和原生窗口使用同一协议。
    )
    .add_systems(Update, (setup, input, style).chain());
    crate::viewport::shading::install_snapshot_controls(&mut app);
    crate::ui::theme::install(&mut app);
    app.finish();
    app.cleanup();
    app.world_mut().spawn((
        crate::components::MapEditorCamera,
        Transform::from_rotation(Quat::from_euler(EulerRot::YXZ, 0.5, -0.4, 0.0)),
    ));
    let target = app.world_mut().resource_mut::<Assets<Image>>().add({
        let mut image = Image::new_target_texture(640, 360, TextureFormat::Bgra8UnormSrgb, None);
        image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
        image
    });
    let camera = app
        .world_mut()
        .spawn((Camera2d, bevy::camera::RenderTarget::from(target.clone())))
        .id();
    let window = app
        .world_mut()
        .spawn((
            PrimaryWindow,
            Window {
                resolution: (640, 360).into(),
                focused: true,
                ..default()
            },
        ))
        .id();
    app.insert_resource(TestPointer { window, camera });
    let root = app
        .world_mut()
        .spawn((
            crate::components::MapEditorRoot,
            bevy::ui::UiTargetCamera(camera),
            Node {
                width: px(528.0 / 1.7),
                height: px(360.0 / 1.7),
                flex_direction: FlexDirection::Column,
                ..default()
            },
        ))
        .id();
    let header = app
        .world_mut()
        .spawn_scene(global_mode_toolbar())
        .unwrap()
        .id();
    app.world_mut().entity_mut(header).insert(ChildOf(root));
    let viewport = app
        .world_mut()
        .spawn((
            MapEditorViewport,
            ChildOf(root),
            Node {
                width: percent(100),
                min_height: px(0),
                flex_grow: 1.0,
                ..default()
            },
        ))
        .id();
    frames(&mut app, 30);
    let (menu, options) = {
        let world = app.world_mut();
        (
            world
                .query_filtered::<Entity, With<ControlsRoot>>()
                .single(world)
                .unwrap(),
            world
                .query::<(Entity, &ModeButton)>()
                .iter(world)
                .find(|(_, button)| matches!(button, ModeButton::Options))
                .unwrap()
                .0,
        )
    };
    assert_eq!(
        app.world().get::<Node>(menu).unwrap().display,
        Display::None
    );
    click(&mut app, options);
    assert_eq!(
        app.world().get::<Node>(menu).unwrap().display,
        Display::Flex
    );
    for mode in [
        ViewportMode::Select,
        ViewportMode::Move,
        ViewportMode::Rotate,
        ViewportMode::Scale,
        ViewportMode::Edit,
    ] {
        let button = {
            let world = app.world_mut();
            world
                .query::<(Entity, &ModeButton)>()
                .iter(world)
                .find(|(_, button)| matches!(button, ModeButton::Mode(value) if *value == mode))
                .unwrap()
                .0
        };
        click(&mut app, button);
        assert_eq!(app.world().resource::<ViewportModeState>().mode, mode);
        assert!(!app.world().resource::<MapEditorState>().dirty);
    } // 五个真实图标沿用原互斥模式动作，点击仅改工具状态，不产生作者数据事务。
    for (mode, name) in [
        (ViewportMode::Select, "viewport-mode-rail-select.png"),
        (ViewportMode::Rotate, "viewport-mode-rail-rotate.png"),
    ] {
        app.world_mut().resource_mut::<ViewportModeState>().mode = mode;
        frames(&mut app, 5);
        let viewport_bounds = bounds(&app, viewport);
        let header_bounds = bounds(&app, header);
        let world = app.world_mut();
        let (node, transform) = world
            .query_filtered::<(&ComputedNode, &UiGlobalTransform), With<ControlsRoot>>()
            .single(world)
            .unwrap();
        let panel = Rect::from_center_size(transform.translation, node.size());
        let (node, transform) = world
            .query_filtered::<(&ComputedNode, &UiGlobalTransform), With<ModeToolsRoot>>()
            .single(world)
            .unwrap();
        let rail = Rect::from_center_size(transform.translation, node.size());
        assert!(rail.min.x >= viewport_bounds.min.x && rail.max.x < panel.min.x);
        assert!(rail.min.y > header_bounds.max.y && rail.max.y <= viewport_bounds.max.y + 1.0);
        assert!((rail.width() - MODE_RAIL_WIDTH * 1.7).abs() <= 1.0);
        for (button, node, transform) in world
            .query::<(&ModeButton, &ComputedNode, &UiGlobalTransform)>()
            .iter(world)
        {
            if node.size().min_element() <= 0.0 {
                continue;
            }
            let rect = Rect::from_center_size(transform.translation, node.size());
            let panel = match button {
                ModeButton::Mode(_) => rail,
                ModeButton::Options => header_bounds,
                _ => panel,
            };
            assert!(rect.min.x >= panel.min.x - 1.0 && rect.max.x <= panel.max.x + 1.0);
            assert!(rect.min.y >= panel.min.y - 1.0 && rect.max.y <= panel.max.y + 1.0);
        }
        snapshot(&mut app, target.clone(), name);
    }
    // 用正式GPU与鼠标释放协议验收新网格工具栏；核心几何事务另有真实ECS覆盖。
    app.init_resource::<crate::authoring::mesh_edit::MeshEditState>()
        .init_resource::<super::super::mesh_edit_ui::MeshAdvancedMenuState>()
        .add_systems(
            Update,
            (
                mesh_tool_input,
                super::super::mesh_edit_ui::input,
                super::super::mesh_edit_ui::style,
            )
                .chain()
                .after(style),
        );
    app.world_mut()
        .resource_mut::<crate::authoring::mesh_edit::MeshEditState>()
        .activate_for_test();
    app.world_mut().resource_mut::<ViewportModeState>().mode = ViewportMode::Select;
    frames(&mut app, 5);
    let edge = {
        let world = app.world_mut();
        world
            .query::<(&Text, &ChildOf)>()
            .iter(world)
            .find(|(text, _)| text.as_str() == "边 2")
            .unwrap()
            .1
            .parent()
    };
    click(&mut app, edge);
    assert!(matches!(
        app.world()
            .resource::<crate::authoring::mesh_edit::MeshEditState>()
            .request,
        Some(crate::authoring::mesh_edit::MeshEditAction::Kind(
            crate::authoring::mesh_edit::MeshElementKind::Edge
        ))
    ));
    app.world_mut()
        .resource_mut::<crate::authoring::mesh_edit::MeshEditState>()
        .kind = crate::authoring::mesh_edit::MeshElementKind::Edge;
    app.world_mut()
        .resource_mut::<crate::authoring::mesh_edit::MeshEditState>()
        .request = None;
    for (mode, tool) in [
        (
            ViewportMode::Move,
            crate::authoring::mesh_edit::MeshEditTool::Move,
        ),
        (
            ViewportMode::Rotate,
            crate::authoring::mesh_edit::MeshEditTool::Rotate,
        ),
        (
            ViewportMode::Scale,
            crate::authoring::mesh_edit::MeshEditTool::Scale,
        ),
        (
            ViewportMode::Select,
            crate::authoring::mesh_edit::MeshEditTool::Select,
        ),
    ] {
        let button = {
            let world = app.world_mut();
            world
                .query::<(Entity, &ModeButton)>()
                .iter(world)
                .find(|(_, button)| matches!(button, ModeButton::Mode(value) if *value == mode))
                .unwrap()
                .0
        };
        click(&mut app, button);
        assert_eq!(
            app.world()
                .resource::<crate::authoring::mesh_edit::MeshEditState>()
                .request,
            Some(crate::authoring::mesh_edit::MeshEditAction::Tool(tool))
        );
        assert_eq!(
            app.world().resource::<ViewportModeState>().mode,
            ViewportMode::Select
        );
        assert!(!app.world().resource::<MapEditorState>().dirty);
        // 该夹具只安装正式UI，回写模拟核心采用意图后检查持续高亮；几何事务有独立ECS回归。
        {
            let mut state = app
                .world_mut()
                .resource_mut::<crate::authoring::mesh_edit::MeshEditState>();
            state.tool = tool;
            state.request = None;
        }
        frames(&mut app, 3);
        assert_eq!(
            app.world().get::<BackgroundColor>(button).unwrap().0,
            colors::SELECT_BG
        );
    }
    for (label, action, kind, selected, advanced) in [
        (
            "框选 B",
            crate::authoring::mesh_edit::MeshEditAction::BoxSelect,
            crate::authoring::mesh_edit::MeshElementKind::Vertex,
            1,
            false,
        ),
        (
            "挤出 E",
            crate::authoring::mesh_edit::MeshEditAction::Extrude,
            crate::authoring::mesh_edit::MeshElementKind::Face,
            1,
            false,
        ),
        (
            "内插面",
            crate::authoring::mesh_edit::MeshEditAction::Inset,
            crate::authoring::mesh_edit::MeshElementKind::Face,
            1,
            true,
        ),
        (
            "倒角",
            crate::authoring::mesh_edit::MeshEditAction::Bevel,
            crate::authoring::mesh_edit::MeshElementKind::Edge,
            1,
            true,
        ),
        (
            "环切",
            crate::authoring::mesh_edit::MeshEditAction::LoopCut,
            crate::authoring::mesh_edit::MeshElementKind::Edge,
            1,
            true,
        ),
        (
            "切割",
            crate::authoring::mesh_edit::MeshEditAction::Knife,
            crate::authoring::mesh_edit::MeshElementKind::Vertex,
            3,
            true,
        ),
        (
            "细分",
            crate::authoring::mesh_edit::MeshEditAction::Subdivide,
            crate::authoring::mesh_edit::MeshElementKind::Face,
            1,
            true,
        ),
    ] {
        app.world_mut()
            .resource_mut::<crate::authoring::mesh_edit::MeshEditState>()
            .select_for_test(kind, selected);
        frames(&mut app, 3);
        if advanced {
            let more = label_button(&mut app, "更多");
            click(&mut app, more);
            frames(&mut app, 2);
        }
        let button = label_button(&mut app, label);
        click(&mut app, button);
        assert_eq!(
            app.world()
                .resource::<crate::authoring::mesh_edit::MeshEditState>()
                .request,
            Some(action)
        );
        app.world_mut()
            .resource_mut::<crate::authoring::mesh_edit::MeshEditState>()
            .request = None;
        assert_eq!(
            app.world().resource::<ViewportModeState>().mode,
            ViewportMode::Select
        );
        assert!(!app.world().resource::<MapEditorState>().dirty);
    } // 新入口同样经过真实鼠标按下/释放，不能用伪造完成事件掩盖菜单命中或裁切问题。
    frames(&mut app, 3);
    snapshot(&mut app, target.clone(), "viewport-mesh-edit-elements.png");
    // 相同正式夹具打开高级区，再检查四球和方向球的实际释放命中，不能仅比较孤立布局。
    let more = label_button(&mut app, "更多");
    click(&mut app, more);
    frames(&mut app, 3);
    snapshot(&mut app, target, "viewport-mesh-edit-advanced-axis-170.png");
    let (axis, shading_buttons) = {
        let world = app.world_mut();
        let axis = world
            .query_filtered::<Entity, With<crate::components::MapEditorOrientationAxisRoot>>()
            .single(world)
            .unwrap();
        let shading = world
            .query::<(Entity, &Name, &InteractionButton)>()
            .iter(world)
            .filter(|(_, name, _)| matches!(name.as_str(), "线框" | "实体" | "材质预览" | "渲染"))
            .map(|(entity, _, _)| entity)
            .collect::<Vec<_>>();
        (axis, shading)
    };
    let axis_rect = bounds(&app, axis);
    let shading_roots = {
        let world = app.world_mut();
        world
            .query_filtered::<Entity, With<crate::viewport::shading::ViewShadingRoot>>()
            .iter(world)
            .filter(|entity| {
                world
                    .get::<ComputedNode>(*entity)
                    .is_some_and(|node| node.size().min_element() > 0.0)
            })
            .collect::<Vec<_>>()
    };
    for root in &shading_roots {
        assert!(
            axis_rect.intersect(bounds(&app, *root)).is_empty(),
            "四球容器和方向球必须各有独立位置"
        );
    }
    let element_buttons = {
        let world = app.world_mut();
        let root = world
            .query_filtered::<Entity, With<crate::viewport::modes::MeshEditToolbarRoot>>()
            .single(world)
            .unwrap();
        world
            .query_filtered::<Entity, With<InteractionButton>>()
            .iter(world)
            .filter(|entity| {
                let mut current = *entity;
                while let Some(parent) = world.get::<ChildOf>(current) {
                    current = parent.parent();
                    if current == root {
                        return true;
                    }
                }
                false
            })
            .collect::<Vec<_>>()
    };
    for button in element_buttons {
        let rect = bounds(&app, button);
        if rect.is_empty() {
            continue;
        }
        assert!(
            rect.intersect(axis_rect).is_empty(),
            "主栏和高级区全部按钮均须避让方向球：{rect:?}，{axis_rect:?}"
        );
        for shading in &shading_roots {
            assert!(
                rect.intersect(bounds(&app, *shading)).is_empty(),
                "建模按钮不得覆盖四球：{rect:?}"
            );
        }
    }
    for label in ["内插面", "切割", "细分"] {
        let button = label_button(&mut app, label);
        let rect = bounds(&app, button);
        assert!(
            rect.intersect(axis_rect).is_empty(),
            "170%高级操作不能被方向球盖住：{label} {rect:?}，方向球{axis_rect:?}"
        );
        click(&mut app, button);
        app.world_mut()
            .resource_mut::<crate::authoring::mesh_edit::MeshEditState>()
            .request = None;
        // 完成操作自动收起高级区；重新打开后继续验证下一按钮。
        click(&mut app, more);
        frames(&mut app, 2);
    }
    assert_eq!(shading_buttons.len(), 4);
    for button in shading_buttons {
        click(&mut app, button);
    }
    let endpoints = {
        let world = app.world_mut();
        world
            .query::<(
                Entity,
                &crate::components::MapEditorOrientationAxisEndpoint,
                &Visibility,
            )>()
            .iter(world)
            .filter(|(_, endpoint, visibility)| {
                endpoint.positive && **visibility != Visibility::Hidden
            })
            .map(|(entity, _, _)| entity)
            .collect::<Vec<_>>()
    };
    for endpoint in endpoints {
        click(&mut app, endpoint);
    }
}

fn label_button(app: &mut App, label: &str) -> Entity {
    let world = app.world_mut();
    world
        .query::<(&Text, &ChildOf)>()
        .iter(world)
        .find(|(text, _)| text.as_str() == label)
        .unwrap()
        .1
        .parent()
}
