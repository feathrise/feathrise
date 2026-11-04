//! 采样契约与数值可视化的纯数据回归；不依赖GPU特性或作者资产。

use super::*;
use bevy::{ecs::system::RunSystemOnce, image::{ImageCompareFunction, ImageSamplerDescriptor},
    render::render_resource::TextureViewDescriptor};
use crate::ui::asset_preview::{presentation::sync_asset_preview_presentation, types::*};

fn source(format: TextureFormat, width: u32, height: u32, data: Vec<u8>) -> Image {
    let mut image = Image::default();
    image.texture_descriptor.format = format;
    image.texture_descriptor.size = Extent3d { width, height, depth_or_array_layers: 1 };
    image.data = Some(data);
    image
}

#[test]
fn uint_preview_normalizes_channels_and_keeps_source_metadata() {
    let bytes: Vec<_> = [0u32, 2, 4, 7, 10, 22, 44, 0].into_iter().flat_map(u32::to_le_bytes).collect();
    let (image, metadata) = visualize(source(TextureFormat::Rgba32Uint, 2, 1, bytes)).unwrap();
    assert!(sample_compatible(&image));
    assert_eq!(image.texture_descriptor.format, TextureFormat::Rgba8UnormSrgb);
    assert_eq!(image.data.as_deref().unwrap(), &[0, 0, 0, 255, 255, 255, 255, 255]);
    assert!(metadata.contains("2 × 1 px · Rgba32Uint"));
    assert!(metadata.contains("数值通道归一化预览"));
}

#[test]
fn compatible_pixels_move_without_clone_or_numeric_preview_label() {
    let image = source(TextureFormat::Rgba8UnormSrgb, 2, 1, vec![3, 5, 7, 11, 13, 17, 19, 23]);
    let pointer = image.data.as_ref().unwrap().as_ptr();
    let (image, metadata) = visualize(image).unwrap();
    assert_eq!(image.data.as_ref().unwrap().as_ptr(), pointer);
    assert_eq!(image.data.as_deref().unwrap(), &[3, 5, 7, 11, 13, 17, 19, 23]);
    assert!(!metadata.contains("归一化"));
}

#[test]
fn signed_float_depth_and_non_finite_values_have_visible_feedback() {
    for (format, data) in [
        (TextureFormat::R16Sint, [-9i16, 15].into_iter().flat_map(i16::to_le_bytes).collect()),
        (TextureFormat::R32Float, [-0.5f32, 3.5].into_iter().flat_map(f32::to_le_bytes).collect()),
        (TextureFormat::Depth16Unorm, [10u16, 50].into_iter().flat_map(u16::to_le_bytes).collect()),
        (TextureFormat::Depth32Float, [0.1f32, 0.9].into_iter().flat_map(f32::to_le_bytes).collect()),
    ] {
        let (image, _) = visualize(source(format, 2, 1, data)).unwrap();
        assert_eq!(image.data.as_deref().unwrap(), &[0, 0, 0, 255, 255, 255, 255, 255], "{format:?}");
    }
    let data = [0.0f32, f32::NAN, f32::INFINITY].into_iter().flat_map(f32::to_le_bytes).collect();
    let (image, _) = visualize(source(TextureFormat::R32Float, 3, 1, data)).unwrap();
    assert_eq!(&image.data.as_deref().unwrap()[4..], &[255, 0, 255, 255, 255, 0, 255, 255]);
}

#[test]
fn numeric_preview_grid_is_bounded_and_preserves_aspect() {
    let (image, _) = visualize(source(TextureFormat::R8Uint, 1024, 256, vec![1; 1024 * 256])).unwrap();
    assert_eq!((image.width(), image.height()), (512, 128));
    assert_eq!(image.data.as_ref().unwrap().len(), 512 * 128 * 4);
    assert!(image.data.as_ref().unwrap().chunks_exact(4).all(|pixel| pixel == [128, 128, 128, 255]));
}

#[test]
fn sampling_contract_checks_usage_view_multisampling_and_sampler() {
    let mut image = Image::default();
    assert!(sample_compatible(&image));
    image.asset_usage = RenderAssetUsages::MAIN_WORLD;
    assert!(!sample_compatible(&image));
    image.asset_usage = RenderAssetUsages::default();
    image.texture_descriptor.usage = TextureUsages::COPY_DST;
    assert!(!sample_compatible(&image));
    image.texture_descriptor.usage |= TextureUsages::TEXTURE_BINDING;
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor { compare: Some(ImageCompareFunction::LessEqual), ..default() });
    assert!(!sample_compatible(&image));
    image.sampler = ImageSampler::Default;
    image.texture_descriptor.sample_count = 4;
    assert!(!sample_compatible(&image));
    image.texture_descriptor.sample_count = 1;
    image.texture_view_descriptor = Some(TextureViewDescriptor { dimension: Some(TextureViewDimension::Cube), ..default() });
    assert!(!sample_compatible(&image));
    image.texture_view_descriptor = None;
    image.texture_descriptor.size.depth_or_array_layers = 2;
    assert!(!sample_compatible(&image));
    image.texture_descriptor.size.depth_or_array_layers = 1;
    image.texture_descriptor.format = TextureFormat::Rgba32Float;
    assert!(!sample_compatible(&image)); // 不假设GPU支持FLOAT32_FILTERABLE。
}

#[test]
fn invalid_or_unsupported_data_retains_actual_format_in_diagnostic() {
    let error = visualize(source(TextureFormat::Rgba32Uint, 2, 1, vec![0; 16])).unwrap_err();
    assert!(error.contains("Rgba32Uint") && error.contains("不完整"));
    let error = visualize(source(TextureFormat::Rgb10a2Uint, 1, 1, vec![0; 4])).unwrap_err();
    assert!(error.contains("Rgb10a2Uint") && error.contains("原格式已保留"));
}

#[test]
fn ready_image_ui_revalidates_label_and_never_binds_unsafe_source() {
    let mut app = App::new();
    let mut images = Assets::<Image>::default();
    let source_handle = images.add(source(TextureFormat::Rgba32Uint, 1, 1, vec![0; 16]));
    let safe = images.add(Image::default());
    app.insert_resource(images).insert_resource(AssetPreviewState {
        selected_path: Some("indices.ktx2".into()),
        content: AssetPreviewContent::Image { handle: source_handle, phase: AssetPreviewPhase::Ready },
        display_image: Some(safe.clone()), ..default()
    });
    let entity = app.world_mut().spawn((AssetPreviewImage, Node::default(), ImageNode::default())).id();
    app.world_mut().run_system_once(sync_asset_preview_presentation).unwrap();
    assert_eq!(app.world().get::<ImageNode>(entity).unwrap().image, safe);
    *app.world_mut().resource_mut::<Assets<Image>>().get_mut(&safe).unwrap() = source(TextureFormat::Rgba32Uint, 1, 1, vec![0; 16]);
    app.world_mut().run_system_once(sync_asset_preview_presentation).unwrap();
    assert_eq!(app.world().get::<ImageNode>(entity).unwrap().image, bevy::image::TRANSPARENT_IMAGE_HANDLE);
    assert_eq!(app.world().get::<Node>(entity).unwrap().display, Display::None);
    *app.world_mut().resource_mut::<Assets<Image>>().get_mut(&safe).unwrap() = Image::default();
    app.world_mut().run_system_once(sync_asset_preview_presentation).unwrap();
    assert_eq!(app.world().get::<ImageNode>(entity).unwrap().image, safe);
}

#[test]
fn failed_numeric_preview_keeps_real_metadata_without_showing_an_old_rgba_image() {
    let mut app = App::new();
    let mut previews = Assets::<CompatiblePreviewTexture>::default();
    let handle = previews.add(CompatiblePreviewTexture {
        image: bevy::image::TRANSPARENT_IMAGE_HANDLE.clone(),
        description: "二维纹理 · 120 × 60 px · Rgb10a2Uint".into(),
        failure: Some("Rgb10a2Uint 无法安全解码".into()),
    });
    app.insert_resource(previews).insert_resource(Assets::<Image>::default())
        .insert_resource(AssetPreviewState {
            selected_path: Some("packed.ktx2".into()), texture_preview: Some(handle),
            content: AssetPreviewContent::Image { handle: Handle::default(), phase: AssetPreviewPhase::Failed },
            ..default()
        });
    let metadata = app.world_mut().spawn((AssetPreviewMetadataText, Node::default(), Text::default())).id();
    app.world_mut().run_system_once(sync_asset_preview_presentation).unwrap();
    assert_eq!(app.world().get::<Text>(metadata).unwrap().as_str(), "二维纹理 · 120 × 60 px · Rgb10a2Uint");
    let id = app.world().resource::<AssetPreviewState>().texture_preview.as_ref().unwrap().id();
    app.world_mut().resource_mut::<Assets<CompatiblePreviewTexture>>().remove(id);
    app.world_mut().run_system_once(sync_asset_preview_presentation).unwrap();
    assert_eq!(app.world().get::<Node>(metadata).unwrap().display, Display::None);
}

mod flow_tests;
