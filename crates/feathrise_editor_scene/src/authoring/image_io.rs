//! 现有场景PNG数值作者源的解码与编码。
//!
//! 【功能设计】旧场景继续直接编辑既有PNG；新场景仍可使用R8+KTX2。两条输入格式只在IO边界
//! 归一成内存R8，笔刷、撤销、预览和正式运行时不复制第二套逻辑。

use std::path::Path; // 扩展名只决定作者容器，不参与密度语义。

use bevy::asset::RenderAssetUsages; // 解码后必须保留CPU数据供编辑草稿读取。
use bevy::image::{CompressedImageFormats, ImageSampler, ImageType}; // 与正式数值纹理保持线性最近采样。
use bevy::prelude::Image; // 复用Bevy已启用的PNG解码能力。
use bevy::render::render_resource::TextureFormat; // 明确校验RGBA8密度与RGBA16高度单元。
use image::ImageEncoder; // Rust原生PNG编码，不调用Python或外部编辑器。

/// 判断作者源是否是可直接绘制的PNG。
pub(super) fn is_png_path(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("png"))
}

fn decode_png(path: &Path, label: &str) -> Result<Image, String> {
    let encoded = std::fs::read(path)
        .map_err(|error| format!("读取{label}{}失败：{error}", path.display()))?;
    Image::from_buffer(
        &encoded,
        ImageType::Extension("png"),
        CompressedImageFormats::NONE,
        false, // 密度和高度都是线性数值，禁止sRGB转换。
        ImageSampler::nearest(),
        RenderAssetUsages::MAIN_WORLD,
    )
    .map_err(|error| format!("解码{label}{}失败：{error}", path.display()))
}

/// 把RGB/RGBA密度PNG的红通道读成编辑器统一R8草稿。
pub(super) fn read_density_png(path: &Path, resolution: [u32; 2]) -> Result<Vec<u8>, String> {
    let image = decode_png(path, "植被密度PNG")?;
    if image.texture_descriptor.format != TextureFormat::Rgba8Unorm
        || image.texture_descriptor.size.width != resolution[0]
        || image.texture_descriptor.size.height != resolution[1]
        || image.texture_descriptor.size.depth_or_array_layers != 1
    {
        return Err(format!(
            "植被密度PNG{}不是声明尺寸的线性8位RGB/RGBA图",
            path.display()
        ));
    }
    let pixels = image
        .data
        .as_deref()
        .ok_or_else(|| format!("植被密度PNG{}没有CPU像素", path.display()))?;
    let expected = resolution[0] as usize * resolution[1] as usize * 4;
    if pixels.len() != expected {
        return Err(format!(
            "植被密度PNG{}解码为{}字节，期望{}字节",
            path.display(),
            pixels.len(),
            expected
        ));
    }
    Ok(pixels.chunks_exact(4).map(|pixel| pixel[0]).collect())
}

/// 从正式NW/NE/SW/SE RGBA16单元PNG重建共享顶点高度，供贴地和编辑预览复用。
pub(super) fn read_height_vertices_png(
    path: &Path,
    terrain_resolution: [u32; 2],
) -> Result<Vec<u16>, String> {
    let [width, height] = terrain_resolution;
    if width < 2 || height < 2 {
        return Err("地形高度分辨率至少为2×2".to_owned());
    }
    let image = decode_png(path, "地形高度单元PNG")?;
    let cell_width = width - 1;
    let cell_height = height - 1;
    if image.texture_descriptor.format != TextureFormat::Rgba16Unorm
        || image.texture_descriptor.size.width != cell_width
        || image.texture_descriptor.size.height != cell_height
    {
        return Err(format!(
            "地形高度单元PNG{}必须为{}×{} RGBA16",
            path.display(),
            cell_width,
            cell_height
        ));
    }
    let data = image
        .data
        .as_deref()
        .ok_or_else(|| format!("地形高度单元PNG{}没有CPU像素", path.display()))?;
    let expected = cell_width as usize * cell_height as usize * 8;
    if data.len() != expected {
        return Err(format!(
            "地形高度单元PNG{}解码为{}字节，期望{}字节",
            path.display(),
            data.len(),
            expected
        ));
    }
    let channel = |cell_x: u32, cell_z: u32, index: usize| -> u16 {
        let offset = ((cell_z * cell_width + cell_x) as usize * 8) + index * 2;
        u16::from_ne_bytes([data[offset], data[offset + 1]])
    };
    let mut heights = Vec::with_capacity(width as usize * height as usize);
    for z in 0..height {
        for x in 0..width {
            heights.push(if x < cell_width && z < cell_height {
                channel(x, z, 0)
            } else if z < cell_height {
                channel(cell_width - 1, z, 1)
            } else if x < cell_width {
                channel(x, cell_height - 1, 2)
            } else {
                channel(cell_width - 1, cell_height - 1, 3)
            });
        }
    }
    Ok(heights)
}

/// 编码为灰度外观的RGB8 PNG；三个颜色通道一致，Windows画图等工具可直接查看和继续编辑。
pub(crate) fn encode_density_png(density: &[u8], resolution: [u32; 2]) -> Result<Vec<u8>, String> {
    let expected = resolution[0] as usize * resolution[1] as usize;
    if density.len() != expected {
        return Err(format!(
            "密度R8为{}字节，声明尺寸需要{}字节",
            density.len(),
            expected
        ));
    }
    let mut rgb = Vec::with_capacity(expected * 3);
    for &value in density {
        rgb.extend_from_slice(&[value, value, value]);
    }
    let mut encoded = Vec::new();
    image::codecs::png::PngEncoder::new(&mut encoded)
        .write_image(
            &rgb,
            resolution[0],
            resolution[1],
            image::ExtendedColorType::Rgb8,
        )
        .map_err(|error| format!("编码植被密度PNG失败：{error}"))?;
    Ok(encoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn density_png_round_trip_keeps_r8_values() {
        let encoded = encode_density_png(&[0, 32, 128, 255], [2, 2]).unwrap();
        let image = Image::from_buffer(
            &encoded,
            ImageType::Extension("png"),
            CompressedImageFormats::NONE,
            false,
            ImageSampler::nearest(),
            RenderAssetUsages::MAIN_WORLD,
        )
        .unwrap();
        let red = image
            .data
            .unwrap()
            .chunks_exact(4)
            .map(|pixel| pixel[0])
            .collect::<Vec<_>>();
        assert_eq!(red, [0, 32, 128, 255]);
    }
}
