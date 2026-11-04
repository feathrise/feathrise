//! 真实GPU检查模式按钮的固定布局及沿边忙碌动画；复用既有离屏读回，不打开用户工程。

use super::{components::*, ui, ViewportMode, ViewportModeState};
use crate::project_log::gpu_tests::{frame, screenshot};
use bevy::{prelude::*, render::render_resource::{TextureFormat, TextureUsages}};
use feathrise_editor_core::{ProjectLaunchButton, ProjectLaunchLabel, ProjectLaunchSettings};
use feathrise_editor_map::{standalone::{SavedPlayFeedback, embedded_snapshot::EmbeddedSnapshotState}, workbench::WorkbenchHeader};
use std::time::Duration;

#[derive(Resource)]
struct TestFont(Handle<Font>);

#[derive(Component)]
struct LaunchGlyph;

fn use_font(font: Res<TestFont>, mut labels: Query<&mut TextFont>) {
    for mut label in &mut labels { label.font = font.0.clone().into(); }
}

fn geometry(app: &mut App) -> Vec<(Entity, Vec2, Vec2)> {
    let world = app.world_mut();
    let mut nodes = world.query_filtered::<(Entity, &ComputedNode, &UiGlobalTransform), Or<(With<ModeButton>, With<ModeLabel>, With<ProjectLaunchButton>, With<ProjectLaunchLabel>, With<LaunchGlyph>)>>()
        .iter(world).map(|(entity, node, transform)| (entity, node.size(), transform.translation)).collect::<Vec<_>>();
    nodes.sort_by_key(|node| node.0);
    nodes
}

#[test]
#[ignore = "显式GPU检查；需设置独占FEATHRISE_MODE_BORDER_GPU_REPORT绝对输出目录"]
fn busy_border_preserves_button_and_text_layout() {
    let output = std::path::PathBuf::from(std::env::var_os("FEATHRISE_MODE_BORDER_GPU_REPORT").expect("必须指定本轮输出目录"));
    assert!(output.is_absolute());
    std::fs::create_dir_all(&output).unwrap();
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin {
        file_path: concat!(env!("CARGO_MANIFEST_DIR"), "/assets").into(),
        watch_for_changes_override: Some(false), ..default()
    }, bevy::scene::ScenePlugin));
    app.add_plugins((WindowPlugin { primary_window: None,
        exit_condition: bevy::window::ExitCondition::DontExit, ..default() },
        bevy::render::RenderPlugin::default(), bevy::image::ImagePlugin::default(),
        bevy::mesh::MeshPlugin, bevy::camera::CameraPlugin, bevy::core_pipeline::CorePipelinePlugin,
        bevy::sprite::SpritePlugin, bevy::sprite_render::SpriteRenderPlugin));
    app.add_plugins((bevy::input::InputPlugin, bevy::input_focus::InputFocusPlugin,
        bevy::input_focus::InputDispatchPlugin, bevy::picking::input::PointerInputPlugin,
        bevy::picking::PickingPlugin, bevy::picking::InteractionPlugin, bevy::transform::TransformPlugin,
        bevy::text::TextPlugin, bevy::ui::UiPlugin, bevy::ui_render::UiRenderPlugin,
        feathrise_editor_ui::EditorUiPlugin));
    app.init_resource::<ViewportModeState>().init_resource::<ProjectLaunchSettings>()
        .init_resource::<crate::native_play::NativePlayStatus>()
        .init_resource::<crate::embedded_viewport::EmbeddedStatus>()
        .init_resource::<EmbeddedSnapshotState>().add_message::<SavedPlayFeedback>()
        // 固定时刻仅用于视觉夹具；生产动画仍读Time<Real>，不改进程就绪合同。
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(Duration::ZERO))
        .add_systems(Update, (ui::mount, ui::mount_launch, use_font, ui::present).chain()
            .after(feathrise_editor_ui::EditorUiSystemSet::NumericInput))
        .add_systems(PostUpdate, (ui::present_connection, ui::present_launch, super::busy_border::animate).chain()
            .before(bevy::ui::UiSystems::Prepare));
    let mut target = Image::new_target_texture(400, 64, TextureFormat::Rgba8UnormSrgb, None);
    target.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let target = app.world_mut().resource_mut::<Assets<Image>>().add(target);
    let camera = app.world_mut().spawn((Camera2d, bevy::camera::RenderTarget::from(target.clone()))).id();
    let header = app.world_mut().spawn((WorkbenchHeader, feathrise_editor_ui::EditorThemeRoot,
        bevy::ui::UiTargetCamera(camera), Node { width: percent(100), height: percent(100),
            align_items: AlignItems::Center, padding: UiRect::all(px(8)), ..default() },
        BackgroundColor(feathrise_editor_ui::colors::PANEL_BG))).id();
    app.world_mut().spawn((Button, ProjectLaunchButton, Node { width: px(64), height: px(26), column_gap: px(4), align_items: AlignItems::Center,
        justify_content: JustifyContent::Center, padding: UiRect::horizontal(px(5)), border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(3)), ..default() },
        BackgroundColor(feathrise_editor_ui::colors::BUTTON_DEV_SMALL_NORMAL), BorderColor::all(feathrise_editor_ui::colors::COPPER_BORDER_WEAK), ChildOf(header)))
        .with_children(|button| {
            button.spawn((LaunchGlyph, Text::new("▶"), TextFont::from_font_size(12.0)));
            button.spawn((ProjectLaunchLabel, Text::new("启动"), TextFont::from_font_size(12.0)));
        });
    app.finish(); app.cleanup();
    let font = app.world_mut().resource_mut::<Assets<Font>>().add(Font::from_bytes(feathrise_runtime::app::default_font::DEFAULT_FONT_BYTES.to_vec()));
    app.insert_resource(TestFont(font));
    for _ in 0..30 { frame(&mut app); }
    let initial = geometry(&mut app);
    assert_eq!(initial.len(), 7, "模式按钮/文字与启动按钮/图标/文字均应实际布局");
    let idle = screenshot(&mut app, &target, &output.join("saved.png"));
    {
        let mut launch = app.world_mut().resource_mut::<crate::native_play::NativePlayStatus>();
        launch.active = true;
        launch.preparing = true;
    }
    for _ in 0..3 { frame(&mut app); }
    let launch_first = screenshot(&mut app, &target, &output.join("launch-busy-top.png"));
    assert_eq!(geometry(&mut app), initial, "启动准备亮段不能移动原按钮、图标、文字或相邻入口");
    let launch_border_size = {
        let world = app.world_mut();
        let (node, border, parent) = world.query_filtered::<(&Node, &ComputedNode, &ChildOf), With<LaunchStatus>>().single(world).unwrap();
        assert_eq!(node.display, Display::Flex);
        assert_eq!(border.size(), world.get::<ComputedNode>(parent.parent()).unwrap().size(), "可见启动亮段应覆盖原按钮边界");
        border.size()
    };
    app.world_mut().resource_mut::<Time<Real>>().advance_by(Duration::from_millis(300));
    for _ in 0..3 { frame(&mut app); }
    let launch_second = screenshot(&mut app, &target, &output.join("launch-busy-side.png"));
    assert_ne!(launch_first, launch_second, "启动亮段必须有真实 GPU 绘制变化");
    assert_eq!(geometry(&mut app), initial);
    app.world_mut().resource_mut::<crate::native_play::NativePlayStatus>().preparing = false;
    for _ in 0..3 { frame(&mut app); }
    let running = screenshot(&mut app, &target, &output.join("launch-running.png"));
    assert!(app.world().resource::<crate::native_play::NativePlayStatus>().active, "运行尚未结束");
    assert_eq!(running, idle, "实际创建运行进程后即结束动画，不能贯穿整个运行期");
    assert_eq!(geometry(&mut app), initial);
    app.world_mut().resource_mut::<ViewportModeState>().selected = ViewportMode::EmbeddedWorld;
    for _ in 0..3 { frame(&mut app); }
    assert_eq!(geometry(&mut app), initial, "切换选中态不改变按钮、文字或后续入口位置");
    screenshot(&mut app, &target, &output.join("embedded-idle.png"));
    app.world_mut().resource_mut::<crate::embedded_viewport::EmbeddedStatus>().active = true;
    for _ in 0..3 { frame(&mut app); }
    let first = screenshot(&mut app, &target, &output.join("busy-top.png"));
    assert_eq!(geometry(&mut app), initial, "显示边框动画不占图标槽、不改变布局");
    app.world_mut().resource_mut::<Time<Real>>().advance_by(Duration::from_millis(300));
    for _ in 0..3 { frame(&mut app); }
    let second = screenshot(&mut app, &target, &output.join("busy-side.png"));
    assert_ne!(first, second, "亮段必须实际沿边移动，不能只改变内部状态");
    assert_eq!(geometry(&mut app), initial);
    let world = app.world_mut();
    for (text, parent) in world.query_filtered::<(&Text, &ChildOf), With<ModeLabel>>().iter(world) {
        assert_eq!(text.as_str(), world.get::<ModeButton>(parent.parent()).unwrap().0.label(), "模式文字不能插入圆点");
    }
    let (border, parent) = world.query_filtered::<(&ComputedNode, &ChildOf), With<ConnectionStatus>>().single(world).unwrap();
    assert_eq!(border.size(), world.get::<ComputedNode>(parent.parent()).unwrap().size(), "动画层应覆盖现有按钮边界");
    app.world_mut().resource_mut::<crate::embedded_viewport::EmbeddedStatus>().ready = true;
    for _ in 0..3 { frame(&mut app); }
    screenshot(&mut app, &target, &output.join("connected.png"));
    assert_eq!(geometry(&mut app), initial, "结束忙碌仍保留相同布局");
    let world = app.world_mut();
    // 已切换方式的启动亮段必须隐藏；Display::None的计算尺寸本来就是零，边界覆盖在上方可见阶段核对。
    let (border_entity, node, border) = world.query_filtered::<(Entity, &Node, &ComputedNode), With<LaunchStatus>>().single(world).unwrap();
    assert_eq!(node.display, Display::None);
    assert_eq!(border.size(), Vec2::ZERO);
    assert_eq!(world.get::<Children>(border_entity).unwrap().len(), 16);
    assert_eq!(world.query_filtered::<&Text, With<ProjectLaunchLabel>>().single(world).unwrap().as_str(), "启动");
    let report = serde_json::json!({"passed":true,"gpu_readback":true,"launch_segments":16,"launch_border_size":[launch_border_size.x,launch_border_size.y],
        "launch_motion_pixels_changed":launch_first != launch_second,"running_pixels_equal_idle":running == idle,
        "geometry_stable":true,"geometry":initial.iter().map(|(entity,size,position)|serde_json::json!({"entity":format!("{entity:?}"),"size":[size.x,size.y],"position":[position.x,position.y]})).collect::<Vec<_>>()});
    std::fs::write(output.join("launch-border.json"), serde_json::to_vec_pretty(&report).unwrap()).unwrap();
}
