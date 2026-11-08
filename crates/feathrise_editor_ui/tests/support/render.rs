//! 各预设库测试共用的真实Bevy二维离屏宿主；只渲染正式表单，不接管原生窗口或启动作者事务。

use bevy::{
    prelude::*,
    render::{
        gpu_readback::{Readback, ReadbackComplete},
        render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages},
    },
};
use std::time::{Duration, Instant};

pub fn app(width: u32, height: u32) -> (App, Entity, Handle<Font>) {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin {
            file_path: concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets").into(),
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
        bevy::picking::input::PointerInputPlugin,
        bevy::picking::PickingPlugin,
        bevy::picking::InteractionPlugin,
        bevy::transform::TransformPlugin,
        bevy::text::TextPlugin,
        bevy::ui::UiPlugin,
        bevy::ui_widgets::TextInputPlugin,
        bevy::ui_render::UiRenderPlugin,
        feathrise_editor_ui::EditorUiPlugin,
    ));
    let mut target = Image::new_target_texture(width, height, TextureFormat::Rgba8UnormSrgb, None);
    target.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let target = app.world_mut().resource_mut::<Assets<Image>>().add(target);
    let camera = app
        .world_mut()
        .spawn((Camera2d, bevy::camera::RenderTarget::from(target)))
        .id();
    let root = app
        .world_mut()
        .spawn((
            feathrise_editor_ui::EditorThemeRoot,
            bevy::ui::UiTargetCamera(camera),
            Node {
                width: percent(100),
                height: percent(100),
                min_width: px(0),
                min_height: px(0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(8)),
                row_gap: px(4),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(feathrise_editor_ui::colors::PANEL_BG),
        ))
        .id();
    app.finish();
    app.cleanup();
    let font = app
        .world_mut()
        .resource_mut::<Assets<Font>>()
        .add(Font::from_bytes(
            feathrise_runtime::app::default_font::DEFAULT_FONT_BYTES.to_vec(),
        ));
    (app, root, font)
}

pub fn clear(app: &mut App, root: Entity) {
    let children = app
        .world()
        .get::<Children>(root)
        .map(|children| children.iter().collect::<Vec<_>>())
        .unwrap_or_default();
    for child in children {
        app.world_mut().entity_mut(child).despawn();
    }
}

pub fn snapshot(app: &mut App, name: &str) {
    for _ in 0..20 {
        app.update();
        std::thread::sleep(Duration::from_millis(16));
    }
    let (target, size) = {
        let world = app.world_mut();
        let target = world
            .query::<&bevy::camera::RenderTarget>()
            .single(world)
            .unwrap();
        let bevy::camera::RenderTarget::Image(target) = target else {
            panic!("只允许读回独立离屏图像")
        };
        let size = world
            .resource::<Assets<Image>>()
            .get(&target.handle)
            .unwrap()
            .size();
        (target.handle.clone(), size)
    };
    let (sender, receiver) = std::sync::mpsc::channel();
    app.world_mut().spawn(Readback::texture(target)).observe(
        move |event: On<ReadbackComplete>, mut commands: Commands| {
            let _ = sender.send(event.data.clone());
            commands.entity(event.entity).try_despawn();
        },
    );
    let started = Instant::now();
    let bytes = loop {
        app.update();
        if let Ok(bytes) = receiver.try_recv() {
            break bytes;
        }
        assert!(started.elapsed().as_secs() < 10, "GPU读回超时");
        std::thread::sleep(Duration::from_millis(16));
    };
    let stride = size.x as usize * 4;
    let pixels = bytes
        .chunks_exact(stride.div_ceil(256) * 256)
        .take(size.y as usize)
        .flat_map(|row| row[..stride].iter().copied())
        .collect();
    let image = Image::new(
        Extent3d {
            width: size.x,
            height: size.y,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        default(),
    );
    let output =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.local/validation");
    std::fs::create_dir_all(&output).unwrap();
    image
        .try_into_dynamic()
        .unwrap()
        .save(output.join(format!("unified-{name}.png")))
        .unwrap();
}
