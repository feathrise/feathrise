//! 显式离屏GPU回归：用原编辑器按钮验证共享帧、主窗合成、官方输入与尺寸换代；不冒充Unix实体窗口验收。

use super::{
    author, host,
    resources::{AuthorSurface, EmbeddedAuthorStatus},
    session::{AuthorSession, HostSession},
};
use bevy::{
    camera::RenderTarget,
    input::{ButtonState, mouse::MouseButtonInput},
    prelude::*,
    render::render_resource::{Extent3d, TextureFormat, TextureUsages},
    window::{CursorMoved, Ime, PrimaryWindow, WindowEvent},
};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

#[derive(Component)]
struct OriginalButton;
#[derive(Resource, Default)]
struct Clicks(u32);
#[derive(Resource, Default)]
struct Committed(Vec<(Entity, String)>);

fn original_ui(mut commands: Commands) {
    commands
        .spawn((
            Node {
                width: percent(100),
                height: percent(100),
                padding: UiRect::all(px(24)),
                ..default()
            },
            BackgroundColor(feathrise_editor_ui::colors::PANEL_BG),
        ))
        .with_children(|parent| {
            let mut style = feathrise_editor_ui::dialog_button_style(true);
            style.node.width = px(180);
            feathrise_editor_ui::spawn_custom_text_button(
                parent,
                OriginalButton,
                Handle::default(),
                "原作者按钮",
                style,
            );
        });
}

fn original_control(
    mut clicks: ResMut<Clicks>,
    buttons: Query<&feathrise_editor_ui::EditorButtonClick, With<OriginalButton>>,
) {
    if buttons.iter().any(|button| button.is_clicked()) {
        clicks.0 += 1;
    }
}

fn committed(mut events: MessageReader<Ime>, mut state: ResMut<Committed>) {
    for event in events.read() {
        if let Ime::Commit { window, value } = event {
            state.0.push((*window, value.clone()));
        }
    }
}

fn app(log: bool) -> App {
    let mut app = App::new();
    let mut plugins = DefaultPlugins
        .build()
        .disable::<bevy::winit::WinitPlugin>()
        .disable::<bevy::render::pipelined_rendering::PipelinedRenderingPlugin>()
        .disable::<bevy::audio::AudioPlugin>()
        .disable::<bevy::app::hotpatch::HotPatchPlugin>()
        .set(AssetPlugin {
            file_path: concat!(env!("CARGO_MANIFEST_DIR"), "/assets").into(),
            watch_for_changes_override: Some(false),
            ..default()
        })
        .set(WindowPlugin {
            primary_window: Some(Window {
                visible: false,
                focused: true,
                resolution: bevy::window::WindowResolution::new(640, 360),
                ..default()
            }),
            exit_condition: bevy::window::ExitCondition::DontExit,
            ..default()
        });
    if !log {
        plugins = plugins.disable::<bevy::log::LogPlugin>();
    }
    app.add_plugins(plugins)
        .add_plugins(feathrise_runtime::DefaultFontPlugin)
        .add_plugins(feathrise_editor_ui::EditorUiPlugin);
    app
}

fn tick(source: &mut App, parent: &mut App, session: &mut HostSession, active: bool) {
    parent.update();
    host::poll(session, parent.world_mut(), active).unwrap();
    source.update();
    std::thread::sleep(Duration::from_millis(12));
}

fn until(
    source: &mut App,
    parent: &mut App,
    session: &mut HostSession,
    active: bool,
    check: impl Fn(&App, &HostSession) -> bool,
) {
    let since = Instant::now();
    while !check(source, session) {
        tick(source, parent, session, active);
        assert!(
            since.elapsed() < Duration::from_secs(45),
            "原作者共享帧/输入回归超时"
        );
    }
}

fn pointer(parent: &mut App, position: Vec2, state: Option<ButtonState>) {
    let mut windows = parent
        .world_mut()
        .query_filtered::<Entity, With<PrimaryWindow>>();
    let window = windows.single(parent.world()).unwrap();
    parent
        .world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .set_cursor_position(Some(position));
    parent
        .world_mut()
        .write_message(WindowEvent::CursorMoved(CursorMoved {
            window,
            position,
            delta: None,
        }));
    if let Some(state) = state {
        parent
            .world_mut()
            .write_message(WindowEvent::MouseButtonInput(MouseButtonInput {
                button: MouseButton::Left,
                state,
                window,
            }));
    }
}

#[test]
#[ignore = "显式portable离屏GPU专项；需本轮独占FEATHRISE_PORTABLE_AUTHOR_GPU_REPORT绝对目录"]
fn original_editor_ui_shared_frames_input_and_resize() {
    let output = PathBuf::from(
        std::env::var_os("FEATHRISE_PORTABLE_AUTHOR_GPU_REPORT")
            .expect("设置本轮独占portable报告目录"),
    );
    assert!(output.is_absolute());
    std::fs::create_dir(&output).expect("报告目录必须是本轮新建，禁止复用旧结果");
    let token = format!(
        "portable-validation-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let mut session = HostSession::new(&token).unwrap();
    let target = session.owned_fixture_target();
    let mut source = app(true);
    source.world_mut().spawn(Camera2d);
    source
        .init_resource::<Clicks>()
        .init_resource::<Committed>()
        .add_systems(
            Update,
            original_control.after(feathrise_editor_ui::update_editor_button_clicks),
        )
        .add_systems(Update, committed);
    source.add_systems(Startup, original_ui);
    author::install_session(
        &mut source,
        AuthorSession::open(&target).unwrap(),
        target.pid,
    );
    let mut parent = app(false);
    host::install(&mut parent);
    let mut image = Image::new_target_texture(640, 360, TextureFormat::Rgba8UnormSrgb, None);
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let screen = parent
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(image);
    parent.world_mut().spawn((
        Camera2d,
        // 真实主窗相机指向Window；离屏夹具须显式登记UI相机，Image目标不会被Bevy自动选为默认。
        bevy::ui::IsDefaultUiCamera,
        RenderTarget::Image(screen.clone().into()),
    ));
    source.finish();
    source.cleanup();
    parent.finish();
    parent.cleanup();
    until(
        &mut source,
        &mut parent,
        &mut session,
        false,
        |source, session| {
            session.ready() && source.world().resource::<EmbeddedAuthorStatus>().ready
        },
    );
    assert_eq!(
        source.world().resource::<Clicks>().0,
        0,
        "隐藏候选不能接收按钮动作"
    );
    host::activate(&mut session, parent.world_mut()).unwrap();
    for _ in 0..12 {
        tick(&mut source, &mut parent, &mut session, true);
    }
    pointer(&mut parent, Vec2::new(80., 38.), Some(ButtonState::Pressed));
    for _ in 0..5 {
        tick(&mut source, &mut parent, &mut session, true);
    }
    pointer(
        &mut parent,
        Vec2::new(80., 38.),
        Some(ButtonState::Released),
    );
    until(&mut source, &mut parent, &mut session, true, |source, _| {
        source.world().resource::<Clicks>().0 == 1
    });
    let mut windows = parent
        .world_mut()
        .query_filtered::<Entity, With<PrimaryWindow>>();
    let parent_window = windows.single(parent.world()).unwrap();
    parent
        .world_mut()
        .write_message(WindowEvent::Ime(Ime::Commit {
            window: parent_window,
            value: "中文输入".into(),
        }));
    until(&mut source, &mut parent, &mut session, true, |source, _| {
        !source.world().resource::<Committed>().0.is_empty()
    });
    let mut windows = source
        .world_mut()
        .query_filtered::<Entity, With<PrimaryWindow>>();
    let source_window = windows.single(source.world()).unwrap();
    assert_eq!(
        source.world().resource::<Committed>().0,
        [(source_window, "中文输入".to_owned())]
    );
    let first_epoch = session.geometry.epoch;
    let first_sequence = session.last_frame;
    parent
        .world_mut()
        .get_mut::<Window>(parent_window)
        .unwrap()
        .resolution
        .set_physical_resolution(520, 320);
    // 实机父相机的swapchain会跟随客户区；离屏夹具同步改GPU目标，不能只改窗口状态冒充完整resize。
    parent
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .get_mut(&screen)
        .unwrap()
        .resize(Extent3d {
            width: 520,
            height: 320,
            depth_or_array_layers: 1,
        });
    until(
        &mut source,
        &mut parent,
        &mut session,
        true,
        |_, session| {
            session.geometry.epoch > first_epoch
                && session.ready()
                && session.last_frame > first_sequence
        },
    );
    assert_eq!(
        (session.geometry.width, session.geometry.height),
        (520, 320)
    );
    for _ in 0..12 {
        tick(&mut source, &mut parent, &mut session, true);
    }
    let screenshot_size = parent
        .world()
        .resource::<Assets<Image>>()
        .get(&screen)
        .unwrap()
        .size();
    assert_eq!(screenshot_size, UVec2::new(520, 320));
    // 分别证明原作者确实渲染了按钮/中文，以及共享帧在父宿主完成合成，不能只凭点击状态算画面通过。
    let source_target = source.world().non_send::<AuthorSurface>().target.clone();
    let source_size = source
        .world()
        .resource::<Assets<Image>>()
        .get(&source_target)
        .unwrap()
        .size();
    assert_eq!(source_size, screenshot_size);
    let source_pixels = crate::project_log::gpu_tests::screenshot(
        &mut source,
        &source_target,
        &output.join("source-original-ui.png"),
    );
    let pixels = crate::project_log::gpu_tests::screenshot(
        &mut parent,
        &screen,
        &output.join("composited-original-ui.png"),
    );
    assert_eq!(
        pixels.len(),
        screenshot_size.x as usize * screenshot_size.y as usize * 4
    );
    std::fs::write(output.join("evidence.json"), serde_json::to_vec_pretty(&serde_json::json!({
        "passed":true,"scope":"headless_gpu_original_editor_widgets_shared_memory_and_official_input",
        "os_input_verified":false,"linux_macos_native_verified":false,"clicks":source.world().resource::<Clicks>().0,
        "ime_commit":"中文输入","final_geometry":[session.geometry.width,session.geometry.height],
        "epoch":session.geometry.epoch,"last_frame":session.last_frame,
        "screenshot_dimensions":[screenshot_size.x,screenshot_size.y],"screenshot_pixels":pixels.len()/4,
        "source_screenshot_dimensions":[source_size.x,source_size.y],"source_screenshot_pixels":source_pixels.len()/4
    })).unwrap()).unwrap();
}
