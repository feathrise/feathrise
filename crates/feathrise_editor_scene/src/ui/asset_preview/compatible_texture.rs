//! 数值纹理的只读预览边界：普通图片直通，非过滤/整数/深度源在后台派生安全RGBA8。
//! 原资产及作者文件不变；UI和缩略图PBR只能引用通过同一采样契约的完整图像。

use bevy::{
    asset::{AssetLoader, LoadContext, RenderAssetUsages, io::Reader},
    image::ImageSampler,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureSampleType, TextureUsages, TextureViewDimension},
    tasks::AsyncComputeTaskPool,
};
use std::sync::atomic::{AtomicUsize, Ordering};

const MAX_EDGE: u32 = 512; // 派生图最多1MiB；主线程只采用结果，不复制或遍历完整源像素。
const MAX_IN_FLIGHT: usize = 2; // 唯一大预览与串行缩略图共享；旧任务真正结束前继续占槽。
const CAPACITY_ERROR: &str = "兼容预览后台槽暂满";
static IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);

struct PreviewPermit;

impl PreviewPermit {
    fn acquire() -> Option<Self> {
        IN_FLIGHT.try_update(Ordering::AcqRel, Ordering::Acquire,
            |active| (active < MAX_IN_FLIGHT).then_some(active + 1)).ok().map(|_| Self)
    }
}

impl Drop for PreviewPermit {
    fn drop(&mut self) { IN_FLIGHT.fetch_sub(1, Ordering::AcqRel); }
}

/// 暂满不是作者文件错误；只让仍然可见的两个客户端在空槽出现后重试，不积压解码快照。
pub(crate) fn retry_capacity(server: &AssetServer, handle: &Handle<CompatiblePreviewTexture>, error: &str) -> bool {
    if !error.contains(CAPACITY_ERROR) { return false; }
    if IN_FLIGHT.load(Ordering::Acquire) < MAX_IN_FLIGHT
        && let Some(path) = handle.path()
    { let _ = server.load::<CompatiblePreviewTexture>(path.clone_owned()); }
    true
}

#[derive(Asset, TypePath, Debug)]
pub(crate) struct CompatiblePreviewTexture {
    pub image: Handle<Image>,
    pub description: String,
    pub failure: Option<String>, // 转换不支持仍发布真实源元数据；画面保持透明，诊断进入底栏。
}

#[derive(Default, TypePath)]
struct CompatiblePreviewTextureLoader;

pub(crate) fn install(app: &mut App) {
    if !app.world().contains_resource::<Assets<CompatiblePreviewTexture>>() {
        app.init_asset::<CompatiblePreviewTexture>()
            .register_asset_loader(CompatiblePreviewTextureLoader);
    }
}

/// 同时检查纹理视图和采样器，不能只按D2或文件扩展名推断普通材质能够采样。
pub(crate) fn sample_compatible(image: &Image) -> bool {
    let descriptor = &image.texture_descriptor;
    let view = image.texture_view_descriptor.as_ref();
    let format = view.and_then(|view| view.format).unwrap_or(descriptor.format);
    let comparison = matches!(&image.sampler, ImageSampler::Descriptor(sampler) if sampler.compare.is_some());
    flat_size(image).is_ok()
        && !comparison
        && image.asset_usage.contains(RenderAssetUsages::RENDER_WORLD)
        && descriptor.usage.contains(TextureUsages::TEXTURE_BINDING)
        && format.sample_type(view.map(|view| view.aspect), None)
            == Some(TextureSampleType::Float { filterable: true })
}

pub(crate) fn flat_size(image: &Image) -> Result<Vec2, String> {
    let descriptor = &image.texture_descriptor;
    let view = image.texture_view_descriptor.as_ref();
    if descriptor.dimension != TextureDimension::D2
        || descriptor.size.depth_or_array_layers != 1
        || !matches!(view.and_then(|view| view.dimension), None | Some(TextureViewDimension::D2))
        || view.is_some_and(|view| view.base_array_layer != 0 || view.array_layer_count.is_some_and(|layers| layers != 1))
        || descriptor.sample_count != 1
        || descriptor.size.width == 0 || descriptor.size.height == 0
    {
        return Err(format!("{:?} · Cube、数组或多重采样纹理不能作为普通二维图片预览", descriptor.format));
    }
    Ok(Vec2::new(descriptor.size.width as f32, descriptor.size.height as f32))
}

impl AssetLoader for CompatiblePreviewTextureLoader {
    type Asset = CompatiblePreviewTexture;
    type Settings = ();
    type Error = std::io::Error;

    async fn load(&self, reader: &mut dyn Reader, _: &(), context: &mut LoadContext<'_>) -> Result<Self::Asset, Self::Error> {
        // 无等待队列：快速换文件最多保留两条解码/计算流水线，暂满由当前客户端稍后重试。
        let permit = PreviewPermit::acquire().ok_or_else(|| std::io::Error::other(CAPACITY_ERROR))?;
        // 借用正式ImageLoader的格式解码和依赖追踪；使用其默认设置，读取/解码位于资产后台任务。
        // take移交整个解码值，禁止为了异步化在帧系统深拷贝Assets<Image>中的Vec。
        let path = context.path().clone_owned();
        let source = context.load_builder().load_value_from_reader::<Image>(path, reader).await
            .map_err(|error| std::io::Error::other(error.to_string()))?.take();
        let original_description = image_description(&source);
        let result = AsyncComputeTaskPool::get().spawn(async move {
            let _permit = permit; // 丢弃loader任务也不能提前放开尚在运行的CPU工作槽。
            visualize(source)
        }).await;
        Ok(match result {
            Ok((image, description)) => CompatiblePreviewTexture {
                image: context.add_labeled_asset("CompatiblePreview".to_owned(), image), description, failure: None,
            },
            Err(failure) => CompatiblePreviewTexture {
                image: bevy::image::TRANSPARENT_IMAGE_HANDLE.clone(), description: original_description, failure: Some(failure),
            },
        })
    }

    // 只允许显式强类型加载；不能覆盖未指定类型时PNG/KTX2的正式ImageLoader选择。
    fn extensions(&self) -> &[&str] { &[] }
}

#[derive(Clone, Copy)]
enum Encoding { Uint, Sint, Float, Depth }

fn encoding(format: TextureFormat) -> Option<(usize, usize, Encoding)> {
    use TextureFormat::*;
    Some(match format {
        R8Uint => (1, 1, Encoding::Uint), Rg8Uint => (2, 1, Encoding::Uint), Rgba8Uint => (4, 1, Encoding::Uint),
        R16Uint => (1, 2, Encoding::Uint), Rg16Uint => (2, 2, Encoding::Uint), Rgba16Uint => (4, 2, Encoding::Uint),
        R32Uint => (1, 4, Encoding::Uint), Rg32Uint => (2, 4, Encoding::Uint), Rgba32Uint => (4, 4, Encoding::Uint),
        R8Sint => (1, 1, Encoding::Sint), Rg8Sint => (2, 1, Encoding::Sint), Rgba8Sint => (4, 1, Encoding::Sint),
        R16Sint => (1, 2, Encoding::Sint), Rg16Sint => (2, 2, Encoding::Sint), Rgba16Sint => (4, 2, Encoding::Sint),
        R32Sint => (1, 4, Encoding::Sint), Rg32Sint => (2, 4, Encoding::Sint), Rgba32Sint => (4, 4, Encoding::Sint),
        R32Float => (1, 4, Encoding::Float), Rg32Float => (2, 4, Encoding::Float), Rgba32Float => (4, 4, Encoding::Float),
        Depth32Float => (1, 4, Encoding::Float), Depth16Unorm => (1, 2, Encoding::Depth),
        Stencil8 => (1, 1, Encoding::Uint),
        _ => return None, // 不猜测Depth24Plus等平台相关打包格式，保留原格式并给出可读诊断。
    })
}

fn number(bytes: &[u8], encoding: Encoding) -> f64 {
    match (encoding, bytes.len()) {
        (Encoding::Uint, 1) => f64::from(bytes[0]),
        (Encoding::Uint, 2) | (Encoding::Depth, 2) => f64::from(u16::from_le_bytes(bytes.try_into().unwrap())),
        (Encoding::Uint, 4) => f64::from(u32::from_le_bytes(bytes.try_into().unwrap())),
        (Encoding::Sint, 1) => f64::from(bytes[0] as i8),
        (Encoding::Sint, 2) => f64::from(i16::from_le_bytes(bytes.try_into().unwrap())),
        (Encoding::Sint, 4) => f64::from(i32::from_le_bytes(bytes.try_into().unwrap())),
        (Encoding::Float, 4) => f64::from(f32::from_le_bytes(bytes.try_into().unwrap())),
        _ => unreachable!(),
    }
}

/// 只采样基础级的有界网格；通道各自归一化，数据纹理的第四通道不冒充透明度。
pub(crate) fn visualize(mut source: Image) -> Result<(Image, String), String> {
    flat_size(&source)?;
    // 私有副本只修复预览所需的上传/采样用途；普通兼容图片的像素、格式和尺寸直接移交。
    source.asset_usage |= RenderAssetUsages::RENDER_WORLD;
    source.texture_descriptor.usage |= TextureUsages::TEXTURE_BINDING;
    if matches!(&source.sampler, ImageSampler::Descriptor(sampler) if sampler.compare.is_some()) {
        source.sampler = ImageSampler::Default;
    }
    let descriptor = &source.texture_descriptor;
    let format = descriptor.format;
    let description = image_description(&source);
    if sample_compatible(&source) { return Ok((source, description)); }
    let description = format!("{description} · 数值通道归一化预览");
    let (channels, component, kind) = encoding(format)
        .ok_or_else(|| format!("{format:?} 无法安全解码为颜色预览；原格式已保留"))?;
    let data = source.data.as_ref().ok_or_else(|| format!("{format:?} 没有可读CPU像素，无法生成兼容预览"))?;
    let stride = channels * component;
    let source_width = descriptor.size.width;
    let source_height = descriptor.size.height;
    let required = u64::from(source_width).checked_mul(u64::from(source_height))
        .and_then(|pixels| pixels.checked_mul(stride as u64))
        .ok_or_else(|| format!("{format:?} 基础级尺寸超出可寻址范围"))?;
    if required > data.len() as u64 { return Err(format!("{format:?} 基础级像素不完整")); }
    let ratio = (MAX_EDGE as f64 / f64::from(source_width.max(source_height))).min(1.0);
    let width = (f64::from(source_width) * ratio).round().max(1.0) as u32;
    let height = (f64::from(source_height) * ratio).round().max(1.0) as u32;
    let mut samples = Vec::with_capacity((width * height) as usize);
    let mut minimum = [f64::INFINITY; 3];
    let mut maximum = [f64::NEG_INFINITY; 3];
    for y in 0..height { for x in 0..width {
        let offset = ((u64::from(y) * u64::from(source_height) / u64::from(height)) * u64::from(source_width)
            + u64::from(x) * u64::from(source_width) / u64::from(width)) as usize * stride;
        let mut sample = [0.0; 3];
        for channel in 0..3 {
            let index = if channels == 1 { 0 } else { channel.min(channels - 1) };
            let begin = offset + index * component;
            sample[channel] = number(&data[begin..begin + component], kind);
            if sample[channel].is_finite() {
                minimum[channel] = minimum[channel].min(sample[channel]);
                maximum[channel] = maximum[channel].max(sample[channel]);
            }
        }
        samples.push(sample);
    }}
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for sample in samples {
        if sample.iter().any(|value| !value.is_finite()) { pixels.extend_from_slice(&[255, 0, 255, 255]); continue; }
        for channel in 0..3 {
            let range = maximum[channel] - minimum[channel];
            let value = if range > 0.0 { (sample[channel] - minimum[channel]) / range }
                else if sample[channel] == 0.0 { 0.0 } else { 0.5 };
            pixels.push((value.clamp(0.0, 1.0) * 255.0).round() as u8);
        }
        pixels.push(255);
    }
    let image = Image::new(Extent3d { width, height, depth_or_array_layers: 1 }, TextureDimension::D2,
        pixels, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default());
    Ok((image, description))
}

fn image_description(image: &Image) -> String {
    let descriptor = &image.texture_descriptor;
    format!("二维纹理 · {} × {} px · {:?}", descriptor.size.width, descriptor.size.height, descriptor.format)
}

#[cfg(test)]
mod tests;
