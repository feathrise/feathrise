//! 使用正式ImageLoader的无GPU加载、容量背压、快速切换与同路径热更回归。

use super::*;
use bevy::{asset::{AssetPlugin, LoadState, LoadedUntypedAsset}, image::{CompressedImageFormats, ImageLoader, ImagePlugin}};
use crate::ui::asset_preview::loading::poll_asset_preview_loading;
use std::{path::PathBuf, time::{Duration, Instant, SystemTime, UNIX_EPOCH}};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("rpg-compatible-preview-{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
}

fn select(app: &mut App, path: &str) {
    let handle = app.world().resource::<AssetServer>().load::<CompatiblePreviewTexture>(path.to_owned());
    *app.world_mut().resource_mut::<AssetPreviewState>() = AssetPreviewState {
        selected_path: Some(path.into()), texture_preview: Some(handle),
        content: AssetPreviewContent::Image { handle: Handle::default(), phase: AssetPreviewPhase::Loading },
        ..default()
    };
}

fn pump(app: &mut App, predicate: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(12);
    loop {
        app.update();
        // 任意中途帧均不可给ImageNode绑定整数/深度/数组图，失败也不能停留上一张画面。
        for entity in app.world().iter_entities() {
            if let Some(image) = entity.get::<ImageNode>() {
                assert!(app.world().resource::<Assets<Image>>().get(&image.image).is_some_and(sample_compatible));
            }
        }
        if predicate(app) { return; }
        assert!(Instant::now() < deadline, "兼容预览没有完成：{:?}", app.world().resource::<AssetPreviewState>());
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn ready(app: &App, expected: &str) -> bool {
    let state = app.world().resource::<AssetPreviewState>();
    matches!(state.content, AssetPreviewContent::Image { phase: AssetPreviewPhase::Ready, .. }) && state.status.contains(expected)
}

#[test]
fn real_image_loader_switch_reload_failure_and_typed_capacity_retry_stay_safe() {
    let fixture = Fixture::new();
    image::RgbaImage::from_pixel(16, 8, image::Rgba([30, 90, 190, 255])).save(fixture.0.join("normal.png")).unwrap();
    let indices: Vec<_> = (0..128).flat_map(|pixel| [if pixel % 16 < 8 { 0u32 } else { 1 }, 2, 3, 0]).flat_map(u32::to_le_bytes).collect();
    let (indices, weights) = feathrise_editor_assets::ktx2::encode_surface_maps(&indices, &[200, 40, 15, 0].repeat(128), [16, 8], 4).unwrap();
    std::fs::write(fixture.0.join("numeric.ktx2"), &indices).unwrap();
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin { file_path: fixture.0.to_string_lossy().into_owned(), ..default() }, ImagePlugin::default()))
        .register_asset_loader(ImageLoader::new(CompressedImageFormats::NONE))
        .init_resource::<AssetPreviewState>()
        .add_systems(Update, (poll_asset_preview_loading, sync_asset_preview_presentation).chain());
    install(&mut app);
    app.finish(); app.cleanup();
    let node = app.world_mut().spawn((AssetPreviewImage, Node::default(), ImageNode::default())).id();
    // 未指定类型的图片请求仍归正式ImageLoader，私有加载器不占据任何图片扩展名。
    let canonical = app.world().resource::<AssetServer>().load_builder().load_untyped("normal.png");
    pump(&mut app, |app| app.world().resource::<Assets<LoadedUntypedAsset>>().get(&canonical).is_some());
    let canonical = app.world().resource::<Assets<LoadedUntypedAsset>>().get(&canonical).unwrap().handle.clone().typed::<Image>();
    let canonical_pointer = app.world().resource::<Assets<Image>>().get(&canonical).unwrap().data.as_ref().unwrap().as_ptr();
    let mut cursor = bevy::ecs::message::MessageCursor::<AssetEvent<Image>>::default();
    cursor.clear(app.world().resource::<Messages<AssetEvent<Image>>>());
    // 两槽都被尚未结束的后台工作占据时，客户端维持Loading；空槽后只重试私有类型。
    let first = PreviewPermit::acquire().unwrap();
    let second = PreviewPermit::acquire().unwrap();
    select(&mut app, "normal.png");
    pump(&mut app, |app| {
        let state = app.world().resource::<AssetPreviewState>();
        matches!(app.world().resource::<AssetServer>().load_state(state.texture_preview.as_ref().unwrap().id()), LoadState::Failed(_))
    });
    assert!(matches!(app.world().resource::<AssetPreviewState>().content, AssetPreviewContent::Image { phase: AssetPreviewPhase::Loading, .. }));
    assert_eq!(app.world().get::<ImageNode>(node).unwrap().image, bevy::image::TRANSPARENT_IMAGE_HANDLE);
    drop((first, second));
    pump(&mut app, |app| ready(app, "Rgba8UnormSrgb"));
    assert_eq!(app.world().resource::<Assets<Image>>().get(&canonical).unwrap().data.as_ref().unwrap().as_ptr(), canonical_pointer);
    assert!(!cursor.read(app.world().resource::<Messages<AssetEvent<Image>>>()).any(|event| matches!(event, AssetEvent::Modified { id } if *id == canonical.id())));

    // 快速在普通图与整数图之间切换，迟到任务只能留在自己的typed父资产，不能换掉当前选择。
    for index in 0..12 {
        select(&mut app, if index % 2 == 0 { "normal.png" } else { "numeric.ktx2" });
        app.update();
    }
    pump(&mut app, |app| ready(app, "Rgba32Uint"));
    assert_eq!(app.world().resource::<AssetPreviewState>().selected_path.as_deref(), Some("numeric.ktx2"));
    assert!(app.world().resource::<AssetServer>().get_handle::<Image>("numeric.ktx2").is_none(), "预览不应再额外请求原Image");
    let safe = app.world().get::<ImageNode>(node).unwrap().image.clone();
    assert_eq!(app.world().resource::<Assets<Image>>().get(&safe).unwrap().texture_descriptor.format, TextureFormat::Rgba8UnormSrgb);
    assert_eq!(std::fs::read(fixture.0.join("numeric.ktx2")).unwrap(), indices);

    for (bytes, format) in [(&weights, "Rgba8Unorm"), (&indices, "Rgba32Uint")] {
        std::fs::write(fixture.0.join("numeric.ktx2"), bytes).unwrap();
        app.world().resource::<AssetServer>().reload("numeric.ktx2");
        pump(&mut app, |app| ready(app, format));
    }
    std::fs::write(fixture.0.join("numeric.ktx2"), b"invalid image").unwrap();
    app.world().resource::<AssetServer>().reload("numeric.ktx2");
    pump(&mut app, |app| matches!(app.world().resource::<AssetPreviewState>().content, AssetPreviewContent::Image { phase: AssetPreviewPhase::Failed, .. }));
    assert_eq!(app.world().get::<ImageNode>(node).unwrap().image, bevy::image::TRANSPARENT_IMAGE_HANDLE);
    std::fs::write(fixture.0.join("numeric.ktx2"), &indices).unwrap();
    app.world().resource::<AssetServer>().reload("numeric.ktx2");
    pump(&mut app, |app| ready(app, "Rgba32Uint"));
}
