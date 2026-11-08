//! 正式底栏的离屏中文画面与真实鼠标回归；不创建原生窗口或写入用户配置。

use feathrise_editor_ui::InteractionButton;
use super::components::*;
use super::resources::EditorStatusBarState;
use super::types::StatusAction;
use super::*;
use crate::components::{
    MapEditorMapLoadingFill, MapEditorMapLoadingInputBlocker, MapEditorMapLoadingPanel,
    MapEditorMapLoadingPercent, MapEditorRoot, MapLoadingRecoveryButton,
};
use crate::resources::{MapEditorNumericScrubMouseGuard, MapEditorState, MapLoadingRecovery};
use crate::ui::{map_loading, map_loading_recovery};
use crate::ui::scene::FONT_PATH;
use crate::ui::theme as colors;
use bevy::ecs::system::RunSystemOnce;
use bevy::input::ButtonState;
use bevy::input::mouse::{MouseButtonInput, MouseScrollUnit, MouseWheel};
use bevy::render::gpu_readback::{Readback, ReadbackComplete};
use bevy::render::render_resource::{TextureFormat, TextureUsages};
use bevy::text::FontSourceTemplate;
use bevy::window::{ExitCondition, PrimaryWindow};
use bevy::winit::WinitPlugin;
use feathrise_editor_core::EditorSettings;
use feathrise_editor_core::EditorColorTheme;
use feathrise_scene_core::state::{ScenePhase, LoadingProgress, LoadingStage, PendingSceneTransition};
use sha2::{Digest, Sha256};
use std::{fs::OpenOptions, path::PathBuf, time::{Duration, Instant}};

#[derive(Resource)]
struct ReportDirectory(PathBuf);

#[derive(Resource)]
struct FixtureFont {
    handle: Handle<Font>,
    sha256: String,
    bytes: usize,
}

/// 精简GPU夹具没有宿主资源绑定；直接登记正式SDK字体，再交给原Text/Parley排版与绘制。
fn use_fixture_font(font: Res<FixtureFont>, mut texts: Query<&mut TextFont>) {
    for mut text in &mut texts {
        if text.font != font.handle.clone().into() { text.font = font.handle.clone().into(); }
    }
}

/// 显式GPU验收只写本轮独占目录，不能继续覆盖.local中的历史图片。
fn report_directory(case: &str) -> PathBuf {
    let root = PathBuf::from(std::env::var_os("FEATHRISE_STATUS_UI_GPU_REPORT")
        .expect("显式指定本轮FEATHRISE_STATUS_UI_GPU_REPORT绝对目录"));
    assert!(root.is_absolute());
    if !root.exists() {
        std::fs::create_dir_all(root.parent().unwrap()).unwrap();
        std::fs::create_dir(&root).unwrap();
        let owner = serde_json::json!({"fixture":"status-dialog-ui-v2","pid":std::process::id()});
        serde_json::to_writer(OpenOptions::new().write(true).create_new(true).open(root.join("owner.json")).unwrap(), &owner).unwrap();
    }
    let owner: serde_json::Value = serde_json::from_slice(&std::fs::read(root.join("owner.json")).unwrap()).unwrap();
    assert_eq!(owner["fixture"], "status-dialog-ui-v2");
    assert_eq!(owner["pid"], std::process::id(), "不能复用旧进程的GPU报告");
    let output = root.join(case);
    std::fs::create_dir(&output).expect("每个GPU场景必须使用本轮新目录");
    output
}

#[derive(Resource)]
struct TestSurface {
    window: Entity,
    camera: Entity,
    target: Handle<Image>,
}

#[derive(Component, Clone, Default)]
struct TestViewport;

#[derive(Component, Clone, Default)]
struct TestWorkbenchAction;

fn test_scene() -> impl Scene {
    bsn! {
        MapEditorRoot
        Node { width: percent(100), height: percent(100), flex_direction: FlexDirection::Column }
        BackgroundColor(colors::PANEL_BG)
        Children [
            (Node { height: px(28), flex_shrink: 0.0, padding: UiRect::horizontal(px(12)), align_items: AlignItems::Center }
                BackgroundColor(colors::PANEL_BG_STRONG) Children [(
                    Text("地图编辑器 · 状态栏验证")
                    TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(12) }
                    TextColor(colors::TEXT_PRIMARY)
                )]),
            (TestViewport Node { width: percent(100), flex_grow: 1.0, min_height: px(0) }
                BackgroundColor(colors::MODEL_PREVIEW_BG)
                Children [(
                    TestWorkbenchAction InteractionButton
                    Node { width: px(80), height: px(20), margin: UiRect::all(px(8)) }
                    BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL)
                )]),
            (@footer()),
            (@map_loading::map_loading_input_blocker()),
            (@popup()),
        ]
    }
}

fn official_hover(world: &mut World) {
    let surface = world.resource::<TestSurface>();
    let (camera, window) = (surface.camera, surface.window);
    let original = world
        .get::<bevy::camera::RenderTarget>(camera)
        .unwrap()
        .clone();
    *world.get_mut::<bevy::camera::RenderTarget>(camera).unwrap() =
        bevy::camera::RenderTarget::Window(bevy::window::WindowRef::Entity(window));
    // 官方命中本身也有按压捕获；缓存同一个系统实例，按下与释放才能属于同一手势。
    world.run_system_cached(bevy::ui::ui_focus_system).unwrap();
    *world.get_mut::<bevy::camera::RenderTarget>(camera).unwrap() = original;
}

fn frames(app: &mut App, count: usize) {
    for _ in 0..count {
        app.update();
        std::thread::sleep(Duration::from_millis(16));
    }
}

fn entity<Marker: Component>(app: &mut App) -> Entity {
    let world = app.world_mut();
    world
        .query_filtered::<Entity, With<Marker>>()
        .single(world)
        .unwrap()
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

fn button(app: &mut App, action: StatusAction) -> Entity {
    let world = app.world_mut();
    world
        .query::<(Entity, &StatusButton)>()
        .iter(world)
        .find(|(_, button)| button.0 == action)
        .unwrap()
        .0
}

fn pointer(app: &mut App, point: Vec2) {
    let window = app.world().resource::<TestSurface>().window;
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .set_physical_cursor_position(Some(point.as_dvec2()));
    frames(app, 3);
}

fn click(app: &mut App, target: Entity) {
    let point = bounds(app, target).center();
    pointer(app, point);
    let window = app.world().resource::<TestSurface>().window;
    app.world_mut().write_message(MouseButtonInput {
        button: MouseButton::Left,
        state: ButtonState::Pressed,
        window,
    });
    frames(app, 1);
    assert_eq!(app.world().get::<Interaction>(target), Some(&Interaction::Pressed), "正式鼠标必须命中底栏按钮");
    app.world_mut().write_message(MouseButtonInput {
        button: MouseButton::Left,
        state: ButtonState::Released,
        window,
    });
    frames(app, 3);
}

fn snapshot(app: &mut App, name: &str) {
    let target = app.world().resource::<TestSurface>().target.clone();
    let image = app
        .world()
        .resource::<Assets<Image>>()
        .get(&target)
        .unwrap();
    let (width, height) = (image.width(), image.height());
    let (sender, receiver) = std::sync::mpsc::channel();
    app.world_mut().spawn(Readback::texture(target)).observe(
        move |event: On<ReadbackComplete>, mut commands: Commands| {
            let _ = sender.send(event.data.clone());
            commands.entity(event.entity).try_despawn();
        },
    );
    let started = Instant::now();
    let mut bytes = loop {
        app.update();
        if let Ok(bytes) = receiver.try_recv() {
            break bytes;
        }
        assert!(started.elapsed().as_secs() < 10, "底栏GPU回读超时");
        std::thread::sleep(Duration::from_millis(16));
    };
    assert_eq!(bytes.len(), width as usize * height as usize * 4);
    for pixel in bytes.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    // 失败也保留本次真实GPU图，不能只在断言成功后留下看似完整的证据。
    let output = app.world().resource::<ReportDirectory>().0.clone();
    let captured = image::RgbaImage::from_raw(width, height, bytes).unwrap();
    let mut file = OpenOptions::new().write(true).create_new(true).open(output.join(name)).unwrap();
    captured.write_to(&mut file, image::ImageFormat::Png).unwrap();
    let bytes = captured.as_raw();
    let popup = entity::<StatusPopup>(app);
    let visible = app.world().get::<Node>(popup).unwrap().display != Display::None;
    let surface = if visible { popup } else { entity::<StatusFooter>(app) };
    let rect = bounds(app, surface);
    assert!(
        rect.min.min_element() >= 0.0 && rect.max.x <= width as f32 && rect.max.y <= height as f32
    );
    let expected = app
        .world()
        .resource::<feathrise_editor_ui::EditorPalette>()
            .resolve(if visible { feathrise_editor_ui::colors::DIALOG_HEADER_BG } else { colors::PANEL_BG_STRONG })
        .to_srgba();
    let logical_scale = app
        .world()
        .get::<ComputedNode>(surface)
        .unwrap()
        .inverse_scale_factor();
    let sample = rect.min + Vec2::splat(4.0 / logical_scale); // 取样避开按UI倍率放大的圆角铜边。
    let offset = (sample.y as usize * width as usize + sample.x as usize) * 4;
    for (actual, expected) in
        bytes[offset..offset + 3]
            .iter()
            .zip([expected.red, expected.green, expected.blue])
    {
        assert!(
            (*actual as f32 - expected * 255.0).abs() <= 4.0,
            "主题背景没有采用：{name}"
        );
    }
    let mut glyph_pixels = 0;
    for vertical in rect.min.y as usize..rect.max.y as usize {
        for horizontal in rect.min.x as usize..rect.max.x as usize {
            let offset = (vertical * width as usize + horizontal) * 4;
            if bytes[offset] > 100 && bytes[offset + 1] > 100 && bytes[offset + 2] > 100 {
                glyph_pixels += 1;
            }
        }
    }
    assert!(glyph_pixels > 50, "中文诊断没有绘制：{name}");
    let mut chrome = if visible {
        let panel_node = app.world().get::<Node>(popup).unwrap();
        let shared = feathrise_editor_ui::dialog_panel_node();
        assert_eq!(panel_node.border, shared.border);
        assert_eq!(panel_node.border_radius, shared.border_radius);
        let root = entity::<MapEditorRoot>(app);
        let root_node = app.world().get::<ComputedNode>(root).unwrap();
        let logical = root_node.size() * root_node.inverse_scale_factor();
        let compact = feathrise_editor_ui::dialog_uses_compact_layout(
            Vec2::new((logical.x - 16.).max(0.), (logical.y * 0.6).min(280.)));
        let header = entity::<StatusPopupHeader>(app);
        let footer = entity::<StatusPopupFooter>(app);
        let expected_header = if compact { feathrise_editor_ui::dialog_compact_header_node() }
            else { feathrise_editor_ui::dialog_header_node() };
        let expected_footer = if compact { feathrise_editor_ui::dialog_compact_footer_node() }
            else { feathrise_editor_ui::dialog_footer_node() };
        assert_eq!(app.world().get::<Node>(header).unwrap(), &expected_header);
        assert_eq!(app.world().get::<Node>(footer).unwrap(), &expected_footer);
        let body = entity::<StatusDetailScroll>(app);
        let body_rect = bounds(app, body);
        assert!(body_rect.width() > 0. && body_rect.height() > 0.);
        let world = app.world_mut();
        let labels = world.query::<(&StatusText, &Text)>().iter(world)
            .filter(|(role, _)| role.0 == super::types::StatusTextRole::Title)
            .map(|(_, value)| value.as_str().to_owned()).collect::<Vec<_>>();
        assert_eq!(labels, vec![String::from("状态详情")], "复制反馈不能替换对话框标题");
        let edge = rect.min + Vec2::new(0.5 / logical_scale, rect.height() * 0.5);
        let inner = Vec2::new(rect.min.x + 4. / logical_scale, edge.y);
        let at = |point: Vec2| (point.y as usize * width as usize + point.x as usize) * 4;
        let (edge_at, inner_at) = (at(edge), at(inner));
        let border = &bytes[edge_at..edge_at + 3];
        let inside = &bytes[inner_at..inner_at + 3];
        assert!(border.iter().zip(inside).any(|(a, b)| a.abs_diff(*b) >= 2),
            "实际GPU外沿须有弱边界，不能退回无边框：{name}");
        assert!(border.iter().copied().max().unwrap() < 180,
            "实际外沿不能再画成高亮闭合框：{name}");
        serde_json::json!({"compact":compact,
            "panel":[rect.min.x,rect.min.y,rect.max.x,rect.max.y],
            "body":[body_rect.min.x,body_rect.min.y,body_rect.max.x,body_rect.max.y],
            "border_pixels":border,"inner_pixels":inside,
            "header_min_height":format!("{:?}",expected_header.min_height),
            "footer_min_height":format!("{:?}",expected_footer.min_height)})
    } else { serde_json::json!({}) };
    if app.world().resource::<MapEditorState>().map_transition_in_progress {
        let viewport_entity = entity::<TestViewport>(app);
        let viewport = bounds(app, viewport_entity);
        let sample = Vec2::new(viewport.center().x, viewport.min.y + 4.0 / logical_scale);
        let offset = (sample.y as usize * width as usize + sample.x as usize) * 4;
        let expected = app.world().resource::<feathrise_editor_ui::EditorPalette>()
            .resolve(colors::MODEL_PREVIEW_BG).to_srgba();
        for (actual, expected) in bytes[offset..offset + 3].iter()
            .zip([expected.red, expected.green, expected.blue]) {
            assert!((*actual as f32 - expected * 255.0).abs() <= 4.0,
                "透明加载输入门不能覆盖工作区底色：{name}");
        }
    }
    let fixture = app.world().resource::<FixtureFont>();
    let font_handle = fixture.handle.clone();
    chrome["font"] = serde_json::json!({"source":"SDK NotoSansCJKsc-VF.otf",
        "sha256":fixture.sha256,"bytes":fixture.bytes,"handle":format!("{:?}",fixture.handle.id())});
    chrome["glyph_pixels"] = glyph_pixels.into();
    chrome["rgba_sha256"] = Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect::<String>().into();
    let world = app.world_mut();
    let glyphs = world.query::<(&StatusText, &Text, &TextFont, &bevy::text::TextLayoutInfo)>().iter(world)
        .filter(|(role, ..)| matches!(role.0, super::types::StatusTextRole::Title | super::types::StatusTextRole::Detail))
        .map(|(role, text, font, layout)| {
            assert_eq!(font.font, font_handle.clone().into(), "诊断必须使用夹具实际登记的SDK字体");
            if visible && role.0 == super::types::StatusTextRole::Title {
                assert!(layout.glyphs.len() >= 4, "中文标题须经过真实字形排版");
            }
            serde_json::json!({"role":format!("{:?}",role.0),"text_bytes":text.len(),"glyphs":layout.glyphs.len()})
        }).collect::<Vec<_>>();
    chrome["text_layouts"] = glyphs.into();
    chrome["passed"] = true.into(); // 只有本次像素、边界与实际字形断言全部完成后才发布成功。
    serde_json::to_writer_pretty(OpenOptions::new().write(true).create_new(true)
        .open(output.join(format!("{name}.json"))).unwrap(), &chrome).unwrap();
}

fn gpu_page(case: &str) -> App {
    let mut app = App::new();
    app.insert_resource(ReportDirectory(report_directory(case)));
    app.add_plugins(
        DefaultPlugins
            .build()
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
    .add_plugins(feathrise_editor_ui::EditorUiPlugin)
    .init_resource::<EditorSettings>()
    .init_resource::<MapEditorNumericScrubMouseGuard>()
    .init_resource::<MapLoadingRecovery>()
    .insert_resource(MapEditorState {
        open: true,
        tool: crate::types::EditorTool::Raise,
        dirty: true,
        ..default()
    });
    let font_bytes = feathrise_runtime::app::default_font::DEFAULT_FONT_BYTES.to_vec();
    let sha256 = Sha256::digest(&font_bytes).iter().map(|byte| format!("{byte:02x}")).collect();
    let bytes = font_bytes.len();
    let handle = app.world_mut().resource_mut::<Assets<Font>>().add(Font::from_bytes(font_bytes));
    app.insert_resource(FixtureFont { handle, sha256, bytes })
        .add_systems(Update, use_fixture_font);
    install(&mut app);
    crate::ui::theme::install(&mut app);
    app.add_systems(
        PreUpdate,
        official_hover
            .after(bevy::ui::UiSystems::Focus)
            .before(feathrise_editor_ui::update_editor_button_clicks)
            .before(systems::track_pointer),
    )
    .add_systems(
        Update,
        (
            map_loading_recovery::handle_recovery_buttons.in_set(crate::MapEditorSystemSet::Input),
            crate::ui::numeric_scrub_mouse_guard::update_map_editor_numeric_scrub_mouse_guard,
        ),
    )
    .add_systems(
        PostUpdate,
        (map_loading::sync_map_loading, map_loading_recovery::sync_recovery_buttons)
            .chain().before(systems::observe_status),
    );
    app.finish();
    app.cleanup();
    let window = app
        .world_mut()
        .spawn((
            Window {
                focused: true,
                ..default()
            },
            PrimaryWindow,
        ))
        .id();
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .resolution
        .set_physical_resolution(1280, 720);
    let mut image = Image::new_target_texture(1280, 720, TextureFormat::Bgra8UnormSrgb, None);
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let target = app.world_mut().resource_mut::<Assets<Image>>().add(image);
    let camera = app
        .world_mut()
        .spawn((Camera2d, bevy::camera::RenderTarget::from(target.clone())))
        .id();
    app.insert_resource(TestSurface {
        window,
        camera,
        target,
    });
    app.world_mut()
        .run_system_once::<_, (), _>(test_scene.spawn())
        .unwrap();
    let root = entity::<MapEditorRoot>(&mut app);
    app.world_mut()
        .entity_mut(root)
        .insert(bevy::ui::UiTargetCamera(camera));
    frames(&mut app, 16);
    app
}

fn recovery_button(app: &mut App, action: MapLoadingRecoveryButton) -> Entity {
    let world = app.world_mut();
    world.query::<(Entity, &MapLoadingRecoveryButton)>().iter(world)
        .find(|(_, button)| **button == action).unwrap().0
}

fn resize(app: &mut App, width: u32, height: u32, scale: f32) {
    let surface = app.world().resource::<TestSurface>();
    let (window, target) = (surface.window, surface.target.clone());
    let mut image = Image::new_target_texture(width, height, TextureFormat::Bgra8UnormSrgb, None);
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    app.world_mut().resource_mut::<Assets<Image>>().insert(target.id(), image).unwrap();
    app.world_mut().get_mut::<Window>(window).unwrap().resolution.set_physical_resolution(width, height);
    app.world_mut().resource_mut::<UiScale>().0 = scale;
    frames(app, 5);
}

fn clear_recovery_request(app: &mut App) {
    // 本夹具只核对正式按钮产生的请求，不安装退役系统或触碰真实地图与资产。
    let mut recovery = app.world_mut().resource_mut::<MapLoadingRecovery>();
    recovery.request = None;
    recovery.epoch = recovery.epoch.wrapping_add(1);
}

#[test]
#[ignore = "显式执行真实GPU加载底栏与鼠标恢复入口；不加载地图、不推进回收事务"]
fn status_bar_gpu_loading_progress_failure_and_recovery_remain_accessible() {
    let mut app = gpu_page("loading");
    let window = app.world().resource::<TestSurface>().window;
    let viewport = entity::<TestViewport>(&mut app);
    let footer = entity::<StatusFooter>(&mut app);
    let content = entity::<StatusFooterContent>(&mut app);
    let panel = entity::<MapEditorMapLoadingPanel>(&mut app);
    let blocker = entity::<MapEditorMapLoadingInputBlocker>(&mut app);
    let author_button = entity::<TestWorkbenchAction>(&mut app);
    let original = (bounds(&app, viewport), bounds(&app, footer));
    let world = app.world_mut();
    let node_count = world.query::<&Node>().iter(world).count();
    let map = feathrise_map_content::MapId::parse("MAP-gpu-loading").unwrap();
    let target_name = "用于加载底栏验收的中文地图".repeat(4);
    let transition = PendingSceneTransition::editor_map(ScenePhase::Active, target_name.clone());
    let mut progress = LoadingProgress::for_transition(&transition);
    progress.stage = LoadingStage::CriticalAssets;
    progress.completed = 76;
    progress.total = 100;
    progress.label = "关键资源准备 · 76 / 100".into();
    {
        let mut editor = app.world_mut().resource_mut::<MapEditorState>();
        editor.selected_map_id = Some(map.clone());
        editor.map_transition_in_progress = true;
    }
    app.insert_resource(progress);
    frames(&mut app, 5);
    assert_eq!(app.world().get::<Node>(content).unwrap().display, Display::None);
    assert_eq!(app.world().get::<Node>(panel).unwrap().display, Display::Flex);
    assert!(app.world().get::<InteractionButton>(blocker).is_none(), "纯输入门不得触发普通按钮的整片背景反馈");
    let fill = entity::<MapEditorMapLoadingFill>(&mut app);
    let percent = entity::<MapEditorMapLoadingPercent>(&mut app);
    assert_eq!(app.world().get::<Node>(fill).unwrap().width, bevy::ui::Val::Percent(58.0));
    assert!(app.world().get::<Text>(percent).unwrap().starts_with("58%"));
    assert_eq!((bounds(&app, viewport), bounds(&app, footer)), original);
    let cancel = recovery_button(&mut app, MapLoadingRecoveryButton::Cancel);
    let retry = recovery_button(&mut app, MapLoadingRecoveryButton::Retry);
    assert_eq!(app.world().get::<Node>(retry).unwrap().display, Display::None);
    let author_position = bounds(&app, author_button).center();
    pointer(&mut app, author_position);
    assert_eq!(app.world().get::<Interaction>(author_button), Some(&Interaction::None));
    app.world_mut().write_message(MouseButtonInput {
        button: MouseButton::Left, state: ButtonState::Pressed, window,
    });
    frames(&mut app, 1);
    assert_eq!(app.world().get::<Interaction>(blocker), Some(&Interaction::Pressed));
    assert_eq!(app.world().get::<Interaction>(author_button), Some(&Interaction::None));
    for theme in [EditorColorTheme::NeutralGray, EditorColorTheme::GraphiteBlue, EditorColorTheme::WarmCopper] {
        app.world_mut().resource_mut::<EditorSettings>().editor_color_theme = theme;
        frames(&mut app, 5);
        snapshot(&mut app, &format!("editor-loading-58-{theme:?}.png"));
    }
    app.world_mut().write_message(MouseButtonInput {
        button: MouseButton::Left, state: ButtonState::Released, window,
    });
    frames(&mut app, 3);
    let toggle = button(&mut app, StatusAction::Toggle);
    let close = button(&mut app, StatusAction::Close);
    click(&mut app, toggle);
    assert!(app.world().resource::<EditorStatusBarState>().popup_open);
    let detail = &app.world().resource::<EditorStatusBarState>().detail;
    assert!(detail.contains(&target_name) && detail.contains("关键资源准备") && detail.contains("58%"));
    assert_eq!((bounds(&app, viewport), bounds(&app, footer)), original);
    snapshot(&mut app, "editor-loading-58-details.png");
    click(&mut app, close);
    click(&mut app, cancel);
    let request = app.world().resource::<MapLoadingRecovery>().request.as_ref().unwrap();
    assert_eq!(request.map_id, map);
    assert!(!request.retry && !request.returning);
    assert!(app.world().resource::<MapEditorState>().dirty);
    assert_eq!((bounds(&app, viewport), bounds(&app, footer)), original);
    clear_recovery_request(&mut app);
    let failure = format!("地形资源加载失败：{}\n完整尾部证据：草稿已保留", "很长的中文资产目录/terrain.glb\n".repeat(40));
    {
        let mut progress = app.world_mut().resource_mut::<LoadingProgress>();
        progress.stage = LoadingStage::Failed;
        progress.label = failure.clone();
    }
    frames(&mut app, 4);
    assert_eq!(app.world().get::<Node>(retry).unwrap().display, Display::Flex);
    click(&mut app, toggle);
    assert!(app.world().resource::<EditorStatusBarState>().popup_open);
    assert!(app.world().resource::<EditorStatusBarState>().detail.contains(&failure));
    snapshot(&mut app, "editor-loading-failed-details.png");
    click(&mut app, close);
    click(&mut app, retry);
    let request = app.world().resource::<MapLoadingRecovery>().request.as_ref().unwrap();
    assert_eq!(request.map_id, map);
    assert!(request.retry && !request.returning);
    clear_recovery_request(&mut app);
    resize(&mut app, 640, 360, 1.5);
    let narrow = (bounds(&app, viewport), bounds(&app, footer));
    click(&mut app, toggle);
    assert!(app.world().resource::<EditorStatusBarState>().popup_open);
    assert_eq!((bounds(&app, viewport), bounds(&app, footer)), narrow);
    snapshot(&mut app, "editor-loading-failed-narrow-150.png");
    click(&mut app, close);
    click(&mut app, retry);
    assert!(app.world().resource::<MapLoadingRecovery>().request.as_ref().unwrap().retry);
    clear_recovery_request(&mut app);
    {
        let mut progress = app.world_mut().resource_mut::<LoadingProgress>();
        progress.stage = LoadingStage::CriticalAssets;
        progress.label = "关键资源准备 · 76 / 100".into();
    }
    frames(&mut app, 4);
    snapshot(&mut app, "editor-loading-58-narrow-150.png");
    click(&mut app, cancel);
    assert!(!app.world().resource::<MapLoadingRecovery>().request.as_ref().unwrap().retry);
    assert_eq!((bounds(&app, viewport), bounds(&app, footer)), narrow);
    app.world_mut().resource_mut::<MapEditorState>().map_transition_in_progress = false;
    app.world_mut().remove_resource::<LoadingProgress>();
    frames(&mut app, 4);
    assert_eq!(app.world().get::<Node>(content).unwrap().display, Display::Flex);
    assert_eq!(app.world().get::<Node>(panel).unwrap().display, Display::None);
    assert_eq!(app.world().get::<Node>(blocker).unwrap().display, Display::None);
    assert_eq!((bounds(&app, viewport), bounds(&app, footer)), narrow);
    let world = app.world_mut();
    assert_eq!(world.query::<&Node>().iter(world).count(), node_count);
    assert!(world.resource::<MapEditorState>().dirty);
    assert!(!world.resource::<MapEditorState>().save_requested);
}

#[test]
#[ignore = "显式执行离屏底栏与详情画面；不启动原生窗口或操作系统剪贴板"]
fn status_bar_gpu_official_pointer_three_themes_and_narrow_scale() {
    let mut app = gpu_page("details");
    let window = app.world().resource::<TestSurface>().window;
    let viewport = entity::<TestViewport>(&mut app);
    let footer = entity::<StatusFooter>(&mut app);
    let original = (bounds(&app, viewport), bounds(&app, footer));
    let world = app.world_mut();
    let node_count = world.query::<&Node>().iter(world).count();
    let error = format!(
        "地图保存失败：无法写入中文路径\n{}\n完整尾部证据：原草稿保留",
        "目录/包含空格的文件节点\n".repeat(40)
    );
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .cancel_close_with_error(error.clone());
    frames(&mut app, 3);
    let toggle = button(&mut app, StatusAction::Toggle);
    click(&mut app, toggle);
    assert!(app.world().resource::<EditorStatusBarState>().popup_open);
    assert!(
        app.world()
            .resource::<EditorStatusBarState>()
            .detail
            .contains(&error)
    );
    assert_eq!((bounds(&app, viewport), bounds(&app, footer)), original);
    for theme in [
        EditorColorTheme::NeutralGray,
        EditorColorTheme::GraphiteBlue,
        EditorColorTheme::WarmCopper,
    ] {
        app.world_mut()
            .resource_mut::<EditorSettings>()
            .editor_color_theme = theme;
        frames(&mut app, 5);
        snapshot(&mut app, &format!("editor-status-{theme:?}.png"));
    }
    let body = entity::<StatusDetailScroll>(&mut app);
    let point = bounds(&app, body).center();
    pointer(&mut app, point);
    app.world_mut().write_message(MouseWheel {
        unit: MouseScrollUnit::Line,
        x: 0.0,
        y: -3.0,
        window,
        phase: bevy::input::touch::TouchPhase::Moved,
    });
    frames(&mut app, 2);
    assert!(app.world().get::<ScrollPosition>(body).unwrap().y > 0.0);
    assert!(
        app.world()
            .resource::<MapEditorNumericScrubMouseGuard>()
            .blocked()
    );
    let scroll = app.world().get::<ScrollPosition>(body).unwrap().y;
    app.world_mut().resource_mut::<MapEditorState>().status = "普通后台预览更新".into();
    frames(&mut app, 2);
    assert_eq!(app.world().get::<ScrollPosition>(body).unwrap().y, scroll);
    assert!(
        app.world()
            .resource::<EditorStatusBarState>()
            .retained
            .is_some()
    );
    let close = button(&mut app, StatusAction::Close);
    click(&mut app, close);
    assert!(!app.world().resource::<EditorStatusBarState>().popup_open);
    assert_eq!((bounds(&app, viewport), bounds(&app, footer)), original);
    let target = app.world().resource::<TestSurface>().target.clone();
    let mut image = Image::new_target_texture(640, 360, TextureFormat::Bgra8UnormSrgb, None);
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    app.world_mut()
        .resource_mut::<Assets<Image>>()
        .insert(target.id(), image)
        .unwrap();
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .resolution
        .set_physical_resolution(640, 360);
    app.world_mut().resource_mut::<UiScale>().0 = 1.5;
    frames(&mut app, 5);
    let original = (bounds(&app, viewport), bounds(&app, footer));
    click(&mut app, toggle);
    assert!(app.world().resource::<EditorStatusBarState>().popup_open);
    assert_eq!((bounds(&app, viewport), bounds(&app, footer)), original);
    snapshot(&mut app, "editor-status-narrow-150.png");
    let world = app.world_mut();
    assert_eq!(world.query::<&Node>().iter(world).count(), node_count);
    assert!(world.resource::<MapEditorState>().dirty);
    assert!(!world.resource::<MapEditorState>().save_requested);
}
