//! 工程日志生产UI的显式离屏GPU回读；输出真实底页和原生拖选截图，不创建用户窗口或接触剪贴板。

use super::{
    LogDecoder, LogSource, ProjectLogPlugin,
    components::{LogAction, LogBody, LogDisplay, LogPanel, LogHeader, LogFooter, LogScroll, LogSearchCount, LogSearchInput, LogToolButton, LogCloseButton, LogStatus},
    resources::{
        CACHE_BYTES, CACHE_LINES, LINE_HEIGHT, ProjectLogChannel, ProjectLogState, VIEW_LINES,
    },
    scene, systems, ui_tests::click_action,
};
use bevy::{
    picking::{
        backend::HitData,
        events::{PointerDrag, Pointer, PointerPress},
        hover::{HoverMap, update_is_hovered},
        pointer::{Location, PointerButton, PointerId},
    },
    prelude::*,
    render::{
        gpu_readback::{Readback, ReadbackComplete},
        render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages},
    },
    text::{EditableText, TextEdit, TextLayoutInfo},
    ui_widgets::{ControlOrientation, TextInputPlugin, Scrollbar, ScrollbarPlugin, ScrollbarThumb},
};
use feathrise_editor_scene::workbench::WorkbenchHeader;
use std::{
    fs::OpenOptions,
    path::Path,
    time::{Duration, Instant},
};

#[derive(Resource)]
struct TestFont(Handle<Font>);

#[derive(Resource, Default)]
struct TestHover(Option<Entity>);

fn provide_hover(input: Res<TestHover>, mut hover: ResMut<HoverMap>) {
    // 仅提供官方拾取层的来源；Hovered传播、生产反馈和最终GPU颜色都不由夹具写入。
    hover.clear();
    if let Some(target) = input.0 {
        hover.entry(PointerId::Mouse).or_default().insert(target, HitData::new(Entity::PLACEHOLDER, 0., None, None));
    }
}

fn toggle_wrap(app: &mut App) {
    click_action(app, LogAction::Wrap);
    for _ in 0..12 { frame(app); }
}

fn use_test_font(font: Res<TestFont>, mut texts: Query<&mut TextFont, (Without<LogBody>, Without<LogDisplay>, Without<TextSpan>)>) {
    for mut text in &mut texts {
        if text.font != font.0.clone().into() {
            text.font = font.0.clone().into();
        }
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

/// 比对生产节点与共享配方，并抽取当前GPU帧中的轮廓和栏背景；几个宿主弹窗共用同一验收门。
/// 业务可决定宽高，边界、留白、层级和配色不能再各自漂移。
pub(crate) fn assert_dialog_chrome(
    app: &App, panel: Entity, header: Entity, footer: Entity, width: usize, pixels: &[u8],
) -> serde_json::Value {
    use feathrise_editor_ui::{colors as c, dialog_panel_node, dialog_header_node, dialog_footer_node};
    let world = app.world();
    let panel_node = world.get::<Node>(panel).unwrap();
    let header_node = world.get::<Node>(header).unwrap();
    let footer_node = world.get::<Node>(footer).unwrap();
    let frame = dialog_panel_node();
    let heading = dialog_header_node();
    let bottom = dialog_footer_node();
    assert_eq!(panel_node.border, frame.border, "弹窗必须使用共享低对比细边界");
    assert_eq!(panel_node.border_radius, frame.border_radius);
    assert_eq!(panel_node.padding, frame.padding, "内边距应属于统一标题、正文和底栏");
    assert_eq!(header_node.min_height, heading.min_height);
    assert_eq!(header_node.border, heading.border);
    assert_eq!(header_node.padding.left, heading.padding.left);
    assert_eq!(header_node.padding.top, heading.padding.top);
    assert_eq!(header_node.padding.bottom, heading.padding.bottom);
    assert_eq!(footer_node.min_height, bottom.min_height);
    assert_eq!(footer_node.border, bottom.border);
    assert_eq!(footer_node.padding, bottom.padding);
    assert_eq!(world.get::<BackgroundColor>(panel).unwrap().0, c::DIALOG_BG);
    assert_eq!(world.get::<BorderColor>(panel).unwrap(), &BorderColor::all(c::DIALOG_BORDER));
    for entity in [header, footer] {
        assert_eq!(world.get::<BackgroundColor>(entity).unwrap().0, c::DIALOG_HEADER_BG);
        assert_eq!(world.get::<BorderColor>(entity).unwrap(), &BorderColor::all(c::DIALOG_BORDER));
    }
    let panel_rect = bounds(app, panel);
    let header_rect = bounds(app, header);
    let footer_rect = bounds(app, footer);
    assert!(header_rect.max.y <= footer_rect.min.y, "标题和固定操作区不能互相覆盖");
    let sample = |point: Vec2| {
        let x = point.x.floor().clamp(0., (width - 1) as f32) as usize;
        let height = pixels.len() / (width * 4);
        let y = point.y.floor().clamp(0., (height - 1) as f32) as usize;
        let at = (y * width + x) * 4;
        [pixels[at], pixels[at + 1], pixels[at + 2], pixels[at + 3]]
    };
    // 左缘中点避开圆角、文字与按钮，真实像素必须体现一条克制的边界，而不是高亮白框。
    let outside = sample(Vec2::new(panel_rect.min.x - 2., panel_rect.center().y));
    let inside = sample(Vec2::new(panel_rect.min.x + 4., panel_rect.center().y));
    // 百分比宽度可能让边缘位于半像素，检查覆盖边界的相邻像素，不能把floor后的窗外像素误作边框。
    let outline = [-0.25, 0.5, 1.25].map(|offset| sample(Vec2::new(panel_rect.min.x + offset, panel_rect.center().y)))
        .into_iter().find(|pixel| *pixel != outside && *pixel != inside)
        .expect("实际GPU帧必须包含区别于窗外和正文的细轮廓像素");
    let header_pixel = sample(Vec2::new(panel_rect.min.x + 4., header_rect.center().y));
    let footer_pixel = sample(Vec2::new(panel_rect.min.x + 4., footer_rect.center().y));
    assert!(outline[..3].iter().all(|channel| *channel < 180), "共享弹窗不能出现高亮白色外框：{outline:?}");
    assert!(header_pixel.iter().zip(footer_pixel).all(|(a, b)| a.abs_diff(b) <= 2),
        "同一弹窗标题与底栏必须使用同一共享背景：{header_pixel:?}/{footer_pixel:?}");
    serde_json::json!({"panel":[panel_rect.min.x,panel_rect.min.y,panel_rect.max.x,panel_rect.max.y],
        "header":[header_rect.min.x,header_rect.min.y,header_rect.max.x,header_rect.max.y],
        "footer":[footer_rect.min.x,footer_rect.min.y,footer_rect.max.x,footer_rect.max.y],
        "outline_rgba":outline,"outside_rgba":outside,"inside_rgba":inside,"header_rgba":header_pixel,"footer_rgba":footer_pixel})
}

fn assert_log_chrome(app: &mut App, width: usize, pixels: &[u8]) -> serde_json::Value {
    let panel = entity::<LogPanel>(app);
    let header = entity::<LogHeader>(app);
    let footer = entity::<LogFooter>(app);
    let report = assert_dialog_chrome(app, panel, header, footer, width, pixels);
    let toolbar = feathrise_editor_ui::dialog_toolbar_header_node();
    let header_node = app.world().get::<Node>(header).unwrap();
    assert_eq!(header_node.padding, toolbar.padding, "日志工具栏固定关闭入口的留白也必须使用共享变体");
    assert_eq!(header_node.flex_wrap, toolbar.flex_wrap);
    assert_eq!(header_node.row_gap, toolbar.row_gap);
    let state = app.world().resource::<ProjectLogState>();
    let toggles = (state.follow, state.wrap);
    let world = app.world_mut();
    let title = world.query::<(&Text, &TextFont)>().iter(world)
        .find(|(text, _)| text.0 == "工程日志").expect("日志标题应与其他弹窗使用同一标题栏");
    assert_eq!(title.1.font_size, feathrise_editor_ui::metrics::FONT_BODY.into());
    for (action, node, colors) in world.query::<(&LogAction, &Node, &feathrise_editor_ui::ButtonColors)>().iter(world) {
        let selected = match action { LogAction::Follow => toggles.0, LogAction::Wrap => toggles.1, _ => false };
        let style = if matches!(action, LogAction::Close) { feathrise_editor_ui::dialog_close_button_style() }
            else { feathrise_editor_ui::dialog_button_style(selected) };
        assert_eq!(node.height, style.node.height);
        assert_eq!(node.border, style.node.border);
        assert_eq!(node.border_radius, style.node.border_radius);
        assert_eq!(colors.normal, style.colors.normal);
    }
    report
}

pub(crate) fn frame(app: &mut App) {
    app.update();
    std::thread::sleep(Duration::from_millis(16));
}

/// PNG只能来自当前实际GPU目标的ReadbackComplete；create_new避免覆盖旧验收图片。
pub(crate) fn screenshot(app: &mut App, target: &Handle<Image>, path: &Path) -> Vec<u8> {
    let size = app
        .world()
        .resource::<Assets<Image>>()
        .get(target)
        .unwrap()
        .size();
    let (sender, receiver) = std::sync::mpsc::channel();
    app.world_mut()
        .spawn(Readback::texture(target.clone()))
        .observe(move |event: On<ReadbackComplete>, mut commands: Commands| {
            let _ = sender.send(event.data.clone());
            commands.entity(event.entity).try_despawn();
        });
    let started = Instant::now();
    let bytes = loop {
        frame(app);
        if let Ok(bytes) = receiver.try_recv() {
            break bytes;
        }
        assert!(
            started.elapsed() < Duration::from_secs(15),
            "工程日志GPU回读超时"
        );
    };
    let stride = size.x as usize * 4;
    let padded_stride = stride.div_ceil(256) * 256;
    assert!(bytes.len() >= padded_stride * size.y as usize);
    let pixels = bytes
        .chunks_exact(padded_stride)
        .take(size.y as usize)
        .flat_map(|row| row[..stride].iter().copied())
        .collect::<Vec<_>>();
    // 布局通过不能替代绘制；空白回读不能被保存并宣称为界面截图。
    let colors = pixels
        .chunks_exact(4)
        .map(|pixel| [pixel[0], pixel[1], pixel[2], pixel[3]])
        .collect::<std::collections::HashSet<_>>();
    assert!(
        colors.len() > 64,
        "GPU回读没有真实UI文字与面板像素：{}种颜色",
        colors.len()
    );
    let image = Image::new(
        Extent3d {
            width: size.x,
            height: size.y,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels.clone(),
        TextureFormat::Rgba8UnormSrgb,
        default(),
    );
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap();
    image
        .try_into_dynamic()
        .unwrap()
        .write_to(
            &mut file,
            bevy::image::ImageFormat::Png
                .as_image_crate_format()
                .unwrap(),
        )
        .unwrap();
    pixels
}

#[test]
#[ignore = "显式GPU截图验收；必须提供本轮独占的FEATHRISE_LOG_UI_GPU_REPORT绝对目录"]
fn production_log_gpu_bottom_and_native_selection_screenshots() {
    let output = std::path::PathBuf::from(
        std::env::var_os("FEATHRISE_LOG_UI_GPU_REPORT")
            .expect("显式设置本轮独占截图目录FEATHRISE_LOG_UI_GPU_REPORT"),
    );
    assert!(output.is_absolute(), "截图目录必须为绝对路径");
    std::fs::create_dir_all(&output).unwrap();
    let bottom_path = output.join("log-bottom.png");
    let selection_path = output.join("log-selection.png");
    let narrow_path = output.join("log-narrow.png");
    let hover_path = output.join("log-hover.png");
    let unwrapped_path = output.join("log-unwrapped.png");
    let rewrapped_path = output.join("log-rewrapped.png");
    let search_path = output.join("project-log-search.png");
    assert!(
        [&bottom_path, &selection_path, &narrow_path, &hover_path, &unwrapped_path, &rewrapped_path, &search_path].iter().all(|path| !path.exists()),
        "禁止覆盖旧验收图片"
    );

    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin {
            file_path: concat!(env!("CARGO_MANIFEST_DIR"), "/assets").into(),
            watch_for_changes_override: Some(false),
            ..default()
        },
        bevy::scene::ScenePlugin,
    ));
    app.add_plugins((
        WindowPlugin {
            primary_window: None,
            exit_condition: bevy::window::ExitCondition::DontExit,
            ..default()
        },
        bevy::render::RenderPlugin::default(),
        bevy::image::ImagePlugin::default(),
        bevy::mesh::MeshPlugin,
        bevy::camera::CameraPlugin,
        bevy::core_pipeline::CorePipelinePlugin,
        bevy::sprite::SpritePlugin,
        bevy::sprite_render::SpriteRenderPlugin,
    ));
    app.add_plugins((
        bevy::input::InputPlugin,
        bevy::input_focus::InputFocusPlugin,
        bevy::input_focus::InputDispatchPlugin,
        bevy::input_focus::pointer_focus::PointerFocusPlugin,
        bevy::picking::input::PointerInputPlugin,
        bevy::picking::PickingPlugin,
        bevy::picking::InteractionPlugin,
        bevy::transform::TransformPlugin,
        bevy::text::TextPlugin,
        feathrise_runtime::DefaultFontPlugin,
        bevy::ui::UiPlugin,
        TextInputPlugin,
        ScrollbarPlugin,
        bevy::ui_render::UiRenderPlugin,
        feathrise_editor_ui::EditorUiPlugin,
    ))
    .add_plugins(ProjectLogPlugin {
        project_id: "日志GPU夹具".into(),
        project_name: "日志GPU夹具".into(),
    })
    .add_systems(Update, bevy::text::apply_text_edits)
    .add_systems(
        Update,
        use_test_font.after(scene::mount).before(systems::present),
    )
    .init_resource::<TestHover>()
    .add_systems(Update, (provide_hover, update_is_hovered).chain().before(super::scrollbar_feedback::sync));
    let mut target = Image::new_target_texture(1280, 720, TextureFormat::Rgba8UnormSrgb, None);
    target.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let target = app.world_mut().resource_mut::<Assets<Image>>().add(target);
    let camera = app
        .world_mut()
        .spawn((Camera2d, bevy::camera::RenderTarget::from(target.clone())))
        .id();
    app.world_mut().spawn((
        WorkbenchHeader,
        feathrise_editor_ui::EditorThemeRoot,
        bevy::ui::UiTargetCamera(camera),
        Node {
            width: percent(100),
            height: percent(100),
            min_width: px(0),
            min_height: px(0),
            ..default()
        },
        BackgroundColor(feathrise_editor_ui::colors::PANEL_BG),
    ));
    app.finish();
    app.cleanup();
    let font = Handle::<Font>::default();
    app.insert_resource(TestFont(font));
    app.world_mut().resource_mut::<ProjectLogState>().open = true;
    let channel = app.world().resource::<ProjectLogChannel>().clone();
    let mut decoder = LogDecoder::new(Some(channel.begin()), LogSource::Stderr);
    let mut payload = (0..120)
        .map(|index| {
            match index % 5 {
                0 => format!("\x1b[1;32m   Compiling\x1b[0m rpg_character_{index:03} v0.1.0\n"),
                1 => format!("\x1b[33mwarning\x1b[0m: 示例诊断第{index:03}行，支持中文拖选和原文复制。\n"),
                2 => format!("\x1b[31merror\x1b[0m: 示例错误第{index:03}行，stderr来源本身不决定颜色。\n"),
                3 => format!("0123456789  ABCDEFGHIJ  {index:03}：等宽列与原生选区逐字对齐。\n"),
                _ => format!("构建日志第{index:03}行：原生只读工程日志，支持中文拖选、复制和滚动。\n"),
            }
        })
        .collect::<String>();
    let live_tail = format!("Building [==================>       ] 119/120：实时未换行尾行 {}", r"D:\worktrees\824b\bevy_learn5_3d\rpg\crates\角色动画\src\preview.rs;".repeat(8));
    payload.push_str(&live_tail);
    // 不调用finish；截图必须证明仍在构建时的临时尾行已经可见。
    decoder.write(payload.as_bytes());
    for _ in 0..40 {
        frame(&mut app);
    }

    let pane = entity::<LogScroll>(&mut app);
    let body = entity::<LogBody>(&mut app);
    let text = app
        .world()
        .get::<EditableText>(body)
        .unwrap()
        .value()
        .to_string();
    let state = app.world().resource::<ProjectLogState>();
    assert_eq!(state.lines.len(), 120);
    assert_eq!(state.row_count(), 121);
    assert!(state.lines.len() <= CACHE_LINES && state.bytes <= CACHE_BYTES);
    assert!(state.wrap && state.display_row_count() > state.row_count());
    assert!(text.lines().count() <= VIEW_LINES && text.ends_with(&live_tail));
    let pane_node = app.world().get::<ComputedNode>(pane).unwrap();
    let transform = app.world().get::<UiGlobalTransform>(pane).unwrap().affine();
    let viewport = Rect::from_corners(
        transform.transform_point2(pane_node.content_box().min),
        transform.transform_point2(pane_node.content_box().max),
    );
    let body_rect = bounds(&app, body);
    assert!(
        body_rect.max.y <= viewport.max.y + 1.1
            && body_rect.max.y - LINE_HEIGHT >= viewport.min.y - 1.1,
        "最新未换行尾行必须完整出现在滚动区：{body_rect:?} / {viewport:?}"
    );
    let bars = {
        let world = app.world_mut();
        world
            .query::<(Entity, &Scrollbar)>()
            .iter(world)
            .map(|(entity, bar)| (entity, bar.target))
            .collect::<Vec<_>>()
    };
    assert_eq!(bars.len(), 2);
    let mut vertical_thumb = None;
    for (track, target) in bars {
        assert_eq!(target, pane);
        if app.world().get::<Scrollbar>(track).unwrap().orientation == ControlOrientation::Horizontal {
            assert_eq!(app.world().get::<Node>(track).unwrap().display, Display::None);
            continue;
        }
        assert!(bounds(&app, track).size().min_element() > 0.0);
        let thumb = app
            .world()
            .get::<Children>(track)
            .unwrap()
            .iter()
            .find(|child| app.world().get::<ScrollbarThumb>(*child).is_some())
            .unwrap();
        assert!(bounds(&app, thumb).size().min_element() > 0.0);
        vertical_thumb = Some(thumb);
    }
    assert_eq!(app.world().get::<ScrollPosition>(pane).unwrap().x, 0.);
    assert!(app.world().get::<TextLayoutInfo>(body).unwrap().size.y <= VIEW_LINES as f32 * LINE_HEIGHT + 1.1);
    let footer = entity::<LogStatus>(&mut app);
    assert!(!app.world().get::<Text>(footer).unwrap().0.contains("日志GPU夹具"));
    let bottom = screenshot(&mut app, &target, &bottom_path);
    let wide_chrome = assert_log_chrome(&mut app, 1280, &bottom);
    // 必须检查真正回读到的彩色字形；解析出样式或生成TextSpan不能代替实际渲染。
    for (label, count) in [
        ("绿色Cargo状态", bottom.chunks_exact(4).filter(|p| p[1] > 105 && p[1] as u16 > p[0] as u16 + 35 && p[1] as u16 > p[2] as u16 + 20).count()),
        ("黄色警告", bottom.chunks_exact(4).filter(|p| p[0] > 130 && p[1] > 110 && p[2] < 100).count()),
        ("红色错误", bottom.chunks_exact(4).filter(|p| p[0] > 130 && p[0] as u16 > p[1] as u16 + 50 && p[0] as u16 > p[2] as u16 + 30).count()),
        ("青色实时进度", bottom.chunks_exact(4).filter(|p| p[1] > 110 && p[2] > 110 && p[0] < 100).count()),
    ] { assert!(count > 10, "{label}实际GPU像素不足：{count}"); }
    let display = entity::<LogDisplay>(&mut app);
    let input_glyphs = &app.world().get::<TextLayoutInfo>(body).unwrap().glyphs;
    let display_glyphs = &app.world().get::<TextLayoutInfo>(display).unwrap().glyphs;
    assert_eq!(input_glyphs.len(), display_glyphs.len(), "字形绘制与原生选区必须来自同一正文");
    for (input, visible) in input_glyphs.iter().zip(display_glyphs) {
        assert!((input.position - visible.position).length() < 0.1,
            "富色文字不能偏离实际原生字形命中：{:?}/{:?}", input.position, visible.position);
    }

    let thumb = vertical_thumb.unwrap();
    let thumb_rect = bounds(&app, thumb);
    app.world_mut().resource_mut::<TestHover>().0 = Some(thumb);
    for _ in 0..4 { frame(&mut app); }
    let hover = screenshot(&mut app, &target, &hover_path);
    let mut brighter = 0;
    for y in (thumb_rect.min.y.ceil() as usize + 2)..(thumb_rect.max.y.floor() as usize - 2) {
        for x in (thumb_rect.min.x.ceil() as usize + 1)..(thumb_rect.max.x.floor() as usize - 1) {
            let at = (y * 1280 + x) * 4;
            if (0..3).all(|channel| hover[at + channel] as u16 > bottom[at + channel] as u16 + 20) { brighter += 1; }
        }
    }
    assert!(brighter > 40, "原生悬停后滑块内部的实际GPU像素未高亮：{brighter}");
    app.world_mut().resource_mut::<TestHover>().0 = None;
    for _ in 0..4 { frame(&mut app); }

    // 坐标来自当前生产正文的真实布局，Pointer事件仍交给官方EditableText字形命中与拖选观察者。
    let start = Vec2::new(body_rect.min.x + 1.0, viewport.min.y + LINE_HEIGHT * 1.5);
    let end = start + Vec2::new(360.0, LINE_HEIGHT * 2.0);
    let render_target = bevy::camera::RenderTarget::from(target.clone())
        .normalize(None)
        .unwrap();
    app.world_mut().trigger(PointerPress {
        entity: body,
        pointer: Pointer::new(PointerId::Mouse, Location {
            target: render_target.clone(),
            position: start,
        }),
        button: PointerButton::Primary,
            hit: HitData::new(camera, 0.0, None, None),
            count: 1,
    });
    frame(&mut app);
    app.world_mut().trigger(PointerDrag {
        entity: body,
        pointer: Pointer::new(PointerId::Mouse, Location {
            target: render_target,
            position: end,
        }),
        button: PointerButton::Primary,
            distance: end - start,
            delta: end - start,
    });
    for _ in 0..12 {
        frame(&mut app);
    }
    let selected = app
        .world()
        .get::<EditableText>(body)
        .unwrap()
        .editor
        .selected_text()
        .expect("原生Pointer拖动必须产生真实选区")
        .to_owned();
    assert!(!selected.is_empty() && selected.chars().any(|character| !character.is_ascii()));
    assert!(
        !app.world()
            .get::<TextLayoutInfo>(body)
            .unwrap()
            .selection_rects
            .is_empty()
    );
    assert_eq!(
        app.world().resource::<ProjectLogState>().copy_text(false),
        selected
    );
    let selection = screenshot(&mut app, &target, &selection_path);
    assert!(
        bottom
            .chunks_exact(4)
            .zip(selection.chunks_exact(4))
            .filter(|(before, after)| before != after)
            .count()
            > 100,
        "两次实际GPU回读未体现选区绘制变化"
    );
    toggle_wrap(&mut app);
    assert!(!app.world().resource::<ProjectLogState>().wrap);
    let horizontal = {
        let world = app.world_mut();
        world.query::<(Entity, &Scrollbar)>().iter(world).find(|(_, bar)| bar.orientation == ControlOrientation::Horizontal).unwrap().0
    };
    assert_eq!(app.world().get::<Node>(horizontal).unwrap().display, Display::Flex);
    assert!(app.world().get::<TextLayoutInfo>(body).unwrap().size.x > viewport.width() + 200.);
    assert_eq!(app.world().resource::<ProjectLogState>().copy_text(false), selected);
    screenshot(&mut app, &target, &unwrapped_path);
    toggle_wrap(&mut app);
    assert!(app.world().resource::<ProjectLogState>().wrap);
    assert_eq!(app.world().get::<Node>(horizontal).unwrap().display, Display::None);
    assert_eq!(app.world().get::<ScrollPosition>(pane).unwrap().x, 0.);
    assert_eq!(app.world().resource::<ProjectLogState>().copy_text(false), selected);
    screenshot(&mut app, &target, &rewrapped_path);
    // 缩窄同一个真实GPU目标，核对动作换行与固定关闭入口，而非另造简化测试布局。
    app.world_mut().resource_mut::<Assets<Image>>().get_mut(&target).unwrap().resize(
        Extent3d { width: 640, height: 720, depth_or_array_layers: 1 },
    );
    for _ in 0..20 { frame(&mut app); }
    let panel = entity::<LogPanel>(&mut app);
    let panel_bounds = bounds(&app, panel);
    let buttons = {
        let world = app.world_mut();
        world.query_filtered::<(Entity, &ComputedNode), With<LogToolButton>>().iter(world)
            .filter(|(_, node)| !node.is_empty()).map(|(entity, _)| entity).collect::<Vec<_>>()
    };
    let close = entity::<LogCloseButton>(&mut app);
    for button in buttons {
        let rect = bounds(&app, button);
        assert!(rect.min.x >= panel_bounds.min.x && rect.max.x <= panel_bounds.max.x + 1.,
            "窄窗动作超出终端面板：{rect:?}/{panel_bounds:?}");
        if button != close { assert!(rect.intersect(bounds(&app, close)).is_empty(),
            "窄窗动作不得盖住固定关闭入口"); }
    }
    let narrow_pixels = screenshot(&mut app, &target, &narrow_path);
    let narrow_chrome = assert_log_chrome(&mut app, 640, &narrow_pixels);
    serde_json::to_writer_pretty(OpenOptions::new().write(true).create_new(true)
        .open(output.join("dialog-chrome.json")).unwrap(),
        &serde_json::json!({"wide":wide_chrome,"narrow":narrow_chrome})).unwrap();
    // 同一个窄窗走生产查找、文字编辑与两种翻页动作，核对实际滚动和背景 GPU 像素。
    click_action(&mut app, LogAction::Search);
    let field = entity::<LogSearchInput>(&mut app);
    let count = entity::<LogSearchCount>(&mut app);
    app.world_mut().get_mut::<EditableText>(field).unwrap().queue_edit(TextEdit::Insert("WaRnInG".into()));
    for _ in 0..12 { frame(&mut app); }
    assert_eq!(app.world().get::<Text>(count).unwrap().0, "1/24 处匹配");
    let first_y = app.world().get::<ScrollPosition>(pane).unwrap().y;
    click_action(&mut app, LogAction::SearchNext);
    for _ in 0..4 { frame(&mut app); }
    assert_eq!(app.world().get::<Text>(count).unwrap().0, "2/24 处匹配");
    for _ in 0..2 {
        click_action(&mut app, LogAction::SearchPrevious);
        for _ in 0..4 { frame(&mut app); }
    }
    assert_eq!(app.world().get::<Text>(count).unwrap().0, "24/24 处匹配");
    let last_y = app.world().get::<ScrollPosition>(pane).unwrap().y;
    assert!(last_y > first_y + 1_000.);
    assert_eq!(app.world().resource::<ProjectLogState>().copy_text(false), selected);
    // 页尾可见的普通命中与之前保留的原生选区叠色。继续用下一处环回首屏，
    // 在原终端背景上分别采样两种查找色，同时保留原选区和上面的复制断言。
    click_action(&mut app, LogAction::SearchNext);
    for _ in 0..4 { frame(&mut app); }
    assert_eq!(app.world().get::<Text>(count).unwrap().0, "1/24 处匹配");
    assert_eq!(app.world().get::<ScrollPosition>(pane).unwrap().y, first_y);
    let searched = screenshot(&mut app, &target, &search_path);
    let pane_rect = bounds(&app, pane);
    let mut active_pixels = 0;
    let mut match_pixels = 0;
    for y in pane_rect.min.y.ceil() as usize..pane_rect.max.y.floor() as usize {
        for x in pane_rect.min.x.ceil() as usize..pane_rect.max.x.floor() as usize {
            let pixel = &searched[(y * 640 + x) * 4..][..3];
            let matches = |rgb: [u8; 3]| pixel.iter().zip(rgb).all(|(value, target)| value.abs_diff(target) < 17);
            if matches([28, 102, 174]) { active_pixels += 1; }
            if matches([113, 76, 22]) { match_pixels += 1; }
        }
    }
    assert!(active_pixels > 100 && match_pixels > 100,
        "GPU 回读应同时包含当前匹配和普通匹配的背景：{active_pixels}/{match_pixels}");
    serde_json::to_writer_pretty(OpenOptions::new().write(true).create_new(true)
        .open(output.join("project-log-search.json")).unwrap(),
        &serde_json::json!({"first_scroll_y":first_y, "last_scroll_y":last_y, "matches":24, "screenshot_active_match":1,
            "active_pixels":active_pixels, "match_pixels":match_pixels,
            "scope":"offscreen production UI, text edit queue, button routing and GPU pixels"})).unwrap();
    click_action(&mut app, LogAction::Search);
    #[cfg(windows)]
    {
        // 沿真实DX展示适配器进入原有界邮箱与生产UI，不直接写日志状态冒充进度折叠。
        app.world_mut().resource_mut::<ProjectLogState>().clear();
        let sink = app.world().resource::<ProjectLogChannel>().begin();
        let mut during = serde_json::Value::Null;
        crate::code_iteration::hotpatch::toolchain::with_fixture_output_log(sink, |emit| {
            for index in 1..=889 {
                let compiling = match index {
                    417 => Some("   Compiling generational-box v0.7.10\n"),
                    418 => Some("   Compiling warnings v0.2.1\n"),
                    419 => Some("   Compiling tracing-subscriber v0.3.23\n"),
                    420 => Some("   Compiling skrifa v0.42.1\n"),
                    _ => None,
                };
                if let Some(text) = compiling { emit(text.as_bytes()); }
                if index == 777 { emit(b"warning: retained diagnostic\nerror[E0308]: retained diagnostic\n"); }
                emit(format!("Compiled [{index}/889]: fixture_{index}\n").as_bytes());
            }
            for _ in 0..16 { frame(&mut app); }
            let state = app.world().resource::<ProjectLogState>();
            assert_eq!(state.lines.len(), 6);
            assert_eq!(state.row_count(), 7, "889次更新只留一条实际可见的进度");
            assert_eq!(state.dropped, 0);
            let copied = state.copy_text(true);
            assert_eq!(copied.matches("Compiled [").count(), 1);
            assert!(copied.ends_with("Compiled [889/889]: fixture_889"));
            let text = app.world().get::<EditableText>(body).unwrap().value().to_string();
            assert_eq!(text.matches("Compiled [").count(), 1, "实际生产正文也只能出现最新进度");
            during = serde_json::json!({"committed_lines":state.lines.len(),"logical_rows":state.row_count(),
                "dropped":state.dropped,"copied_text":copied,"rendered_text":text});
            screenshot(&mut app, &target, &output.join("log-compiled-progress.png"));
        });
        for _ in 0..12 { frame(&mut app); }
        let state = app.world().resource::<ProjectLogState>();
        let copied = state.copy_text(true);
        assert_eq!(state.row_count(), 6, "结束后临时进度撤回，不追加Compiled历史");
        assert!(!copied.contains("Compiled ["));
        assert!(copied.contains("warning: retained diagnostic") && copied.contains("error[E0308]: retained diagnostic"));
        let proof = serde_json::json!({"progress_records":889,"during":during,"after":{"logical_rows":state.row_count(),
            "dropped":state.dropped,"copied_text":copied},"source":"DX OutputLog -> bounded ProjectLogChannel -> production UI -> ReadbackComplete",
            "scope":"offscreen production GPU UI; no physical keyboard or mouse"});
        screenshot(&mut app, &target, &output.join("log-compiled-finished.png"));
        serde_json::to_writer_pretty(OpenOptions::new().write(true).create_new(true)
            .open(output.join("compiled-progress.json")).unwrap(), &proof).unwrap();
    }
    println!("FEATHRISE_LOG_UI_GPU_REPORT {}", output.display());
}
