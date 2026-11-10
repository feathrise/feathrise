//! 使用正式共享半透明立方体实体生成真实离屏GPU快照，不启动游戏窗口或写作者文件。

use super::{components::*, *};
use bevy::{
    camera::visibility::RenderLayers,
    prelude::*,
    render::{
        gpu_readback::{Readback, ReadbackComplete},
        render_resource::{TextureFormat, TextureUsages},
    },
    window::ExitCondition,
    winit::WinitPlugin,
};

#[test]
#[ignore = "需要真实GPU；基础形状半透明预览离屏验收"]
fn cuboid_translucent_preview_gpu_snapshot() {
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
    .insert_resource(ClearColor(Color::srgb(0.11, 0.13, 0.16)))
    .add_systems(Startup, placement::setup_preview);
    crate::ui::theme::install(&mut app);
    app.finish();
    app.cleanup();
    let target = app.world_mut().resource_mut::<Assets<Image>>().add({
        let mut image = Image::new_target_texture(1280, 720, TextureFormat::Bgra8UnormSrgb, None);
        image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
        image
    });
    let layer = RenderLayers::layer(crate::viewport::SCENE_EDITOR_BRUSH_RENDER_LAYER);
    app
        .world_mut()
        .spawn((
            Camera3d::default(),
            bevy::camera::RenderTarget::from(target.clone()),
            Transform::from_xyz(6.0, 5.0, 7.0).looking_at(Vec3::Y, Vec3::Y),
            layer.clone(),
        ));
    let plane = app
        .world_mut()
        .resource_mut::<Assets<Mesh>>()
        .add(Plane3d::default().mesh().size(20.0, 20.0));
    let material = app
        .world_mut()
        .resource_mut::<Assets<StandardMaterial>>()
        .add(StandardMaterial {
            base_color: Color::srgb(0.20, 0.23, 0.27),
            unlit: true,
            ..default()
        });
    app.world_mut()
        .spawn((Mesh3d(plane), MeshMaterial3d(material), layer));
    app.update();
    {
        let world = app.world_mut();
        let (mut transform, mut visible) = world
            .query_filtered::<(&mut Transform, &mut Visibility), With<CuboidGhost>>()
            .single_mut(world)
            .unwrap();
        transform.translation = Vec3::Y;
        *visible = Visibility::Inherited;
    }
    let start = std::time::Instant::now();
    loop {
        app.update();
        if start.elapsed().as_millis() > 750 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
    let (sender, receiver) = std::sync::mpsc::channel();
    app.world_mut().spawn(Readback::texture(target)).observe(
        move |event: On<ReadbackComplete>, mut commands: Commands| {
            let _ = sender.send(event.data.clone());
            commands.entity(event.entity).try_despawn();
        },
    );
    let start = std::time::Instant::now();
    let mut bytes = loop {
        app.update();
        if let Ok(bytes) = receiver.try_recv() {
            break bytes;
        }
        assert!(start.elapsed().as_secs() < 10, "基础形状GPU快照回读超时");
        std::thread::sleep(std::time::Duration::from_millis(16));
    };
    assert_eq!(bytes.len(), 1280 * 720 * 4);
    for pixel in bytes.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    let directory =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.local/validation");
    std::fs::create_dir_all(&directory).unwrap();
    image::RgbaImage::from_raw(1280, 720, bytes)
        .unwrap()
        .save(directory.join("cuboid-translucent-preview.png"))
        .unwrap();
}
