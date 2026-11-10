//! 编辑器内置的未压缩KTX2派生编码器。
//!
//! 【功能设计】作者草稿保存简单 R8/RGBA8/RGBA32/R16 源，游戏只加载带完整 Mip 的 KTX2。
//! 地表索引与权重必须由同一次聚合生成匹配 Mip：每级统计上级覆盖的全部材质，
//! 稳定选取贡献最高的四项再归一化，因而远景不会把索引和权重分别线性过滤成无意义组合。

use std::collections::BTreeMap;

const KTX2_MAGIC: &[u8; 12] = b"\xABKTX 20\xBB\r\n\x1A\n";
const HEADER_BYTES: usize = 80;
const LEVEL_INDEX_BYTES: usize = 24;
const VK_FORMAT_R8_UNORM: u32 = 9;
const VK_FORMAT_R8G8B8A8_UNORM: u32 = 37;
const VK_FORMAT_R8G8B8A8_SRGB: u32 = 43;
const VK_FORMAT_R16G16B16A16_UNORM: u32 = 91;
const VK_FORMAT_R32G32B32A32_UINT: u32 = 107;

/// 二维数组的内容语义决定 Mip 过滤方式，不能把颜色、数据和法线都按字节平均。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rgba8ArrayKind {
    /// RGB 先转线性空间平均再编码回 sRGB；Alpha 始终线性平均。
    SrgbColor,
    /// AO、粗糙度、金属度和高度等线性标量逐通道平均。
    LinearData,
    /// RGB 解码为切线空间方向，平均后重新归一化；Alpha 线性平均。
    TangentNormal,
}

impl Rgba8ArrayKind {
    const fn is_srgb(self) -> bool {
        matches!(self, Self::SrgbColor)
    }
}

#[derive(Clone)]
struct MipLevel {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

fn push_u16(output: &mut Vec<u8>, value: u16) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn push_u64(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn align_up(value: usize, alignment: usize) -> usize {
    value.div_ceil(alignment) * alignment
}

fn validate_image_source(
    resolution: [u32; 2],
    pixels: usize,
    channels: usize,
) -> Result<(), String> {
    let [width, height] = resolution;
    if width == 0 || height == 0 {
        return Err(format!("KTX2尺寸必须非零：{width}×{height}"));
    }
    let expected = usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|texels| texels.checked_mul(channels))
        .ok_or_else(|| format!("KTX2源尺寸溢出：{width}×{height}×{channels}"))?;
    if pixels != expected {
        return Err(format!("KTX2源数据为{pixels}字节，期望{expected}字节"));
    }
    Ok(())
}

/// 构造KHR Data Format Descriptor；当前只编码GPU可直接上传的UNORM格式。
fn build_dfd(
    bytes_per_texel: u8,
    bits_per_channel: u8,
    channels: &[u8],
    upper: u32,
    srgb: bool,
) -> Vec<u8> {
    let block_size = 24 + channels.len() * 16;
    let mut block = Vec::with_capacity(block_size + 4);
    push_u32(&mut block, (block_size + 4) as u32);
    push_u32(&mut block, 0); // vendorId与descriptorType均为KHR标准零。
    push_u16(&mut block, 2); // descriptor version 1.3。
    push_u16(&mut block, block_size as u16);
    block.extend_from_slice(&[
        1,                        // color model: RGBSDA
        1,                        // primaries: BT.709
        if srgb { 2 } else { 1 }, // KHR_DF_TRANSFER_SRGB 或 LINEAR。
        0,                        // flags
        0,
        0,
        0,
        0, // texel block dimensions减一，普通二维单纹素块均为零
        bytes_per_texel,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
    ]);
    for (index, channel) in channels.iter().copied().enumerate() {
        push_u16(&mut block, index as u16 * (bits_per_channel as u16 + 1));
        block.push(bits_per_channel);
        block.push(channel);
        block.extend_from_slice(&[0, 0, 0, 0]);
        push_u32(&mut block, 0);
        push_u32(&mut block, upper);
    }
    debug_assert_eq!(block.len(), block_size + 4);
    block
}

fn encode_uncompressed(
    vk_format: u32,
    type_size: u32,
    dfd: &[u8],
    levels: &[MipLevel],
    layer_count: u32,
) -> Result<Vec<u8>, String> {
    let base = levels
        .first()
        .ok_or_else(|| "KTX2至少需要一个Mip".to_owned())?;
    let dfd_offset = HEADER_BYTES + levels.len() * LEVEL_INDEX_BYTES;
    let mut cursor = align_up(dfd_offset + dfd.len(), 8);
    let mut offsets = Vec::with_capacity(levels.len());
    for level in levels {
        if level.width == 0 || level.height == 0 || level.pixels.is_empty() {
            return Err("KTX2包含空Mip".to_owned());
        }
        cursor = align_up(cursor, 8);
        offsets.push(cursor);
        cursor = cursor
            .checked_add(level.pixels.len())
            .ok_or_else(|| "KTX2文件尺寸溢出".to_owned())?;
    }

    let mut output = Vec::with_capacity(cursor);
    output.extend_from_slice(KTX2_MAGIC);
    for value in [
        vk_format,
        type_size,
        base.width,
        base.height,
        0,           // pixelDepth
        layer_count, // 普通二维纹理为零；二维数组使用真实切片数。
        1,           // faceCount
        levels.len() as u32,
        0, // 无supercompression
        dfd_offset as u32,
        dfd.len() as u32,
        0, // kvdByteOffset
        0, // kvdByteLength
    ] {
        push_u32(&mut output, value);
    }
    push_u64(&mut output, 0); // sgdByteOffset
    push_u64(&mut output, 0); // sgdByteLength
    debug_assert_eq!(output.len(), HEADER_BYTES);
    for (level, offset) in levels.iter().zip(&offsets) {
        push_u64(&mut output, *offset as u64);
        push_u64(&mut output, level.pixels.len() as u64);
        push_u64(&mut output, level.pixels.len() as u64);
    }
    output.extend_from_slice(dfd);
    for (level, offset) in levels.iter().zip(offsets) {
        output.resize(offset, 0);
        output.extend_from_slice(&level.pixels);
    }
    Ok(output)
}

fn downsample_r8(source: &MipLevel) -> MipLevel {
    // KTX/WGPU要求每级尺寸向下折半；按比例分区采样，仍把奇数尺寸最外侧行列纳入平均。
    let width = (source.width / 2).max(1);
    let height = (source.height / 2).max(1);
    let mut pixels = Vec::with_capacity(width as usize * height as usize);
    for y in 0..height {
        for x in 0..width {
            let mut sum = 0u32;
            let mut count = 0u32;
            let source_x_start = x * source.width / width;
            let source_x_end = (x + 1) * source.width / width;
            let source_y_start = y * source.height / height;
            let source_y_end = (y + 1) * source.height / height;
            for source_y in source_y_start..source_y_end {
                for source_x in source_x_start..source_x_end {
                    sum += source.pixels[(source_y * source.width + source_x) as usize] as u32;
                    count += 1;
                }
            }
            pixels.push(((sum + count / 2) / count) as u8);
        }
    }
    MipLevel {
        width,
        height,
        pixels,
    }
}

/// 把四个非负贡献归一到严格合计 255，并按槽顺序稳定分配整数余数。
fn normalize_four(weights: [u64; 4]) -> [u8; 4] {
    let mut weights = weights;
    let mut total = weights.iter().copied().sum::<u64>();
    if total == 0 {
        weights[0] = 255;
        total = 255;
    }
    let mut encoded = [0u8; 4];
    let mut remainders = [0u64; 4];
    let mut assigned = 0u32;
    for index in 0..4 {
        let numerator = weights[index] * 255;
        encoded[index] = (numerator / total) as u8;
        remainders[index] = numerator % total;
        assigned += encoded[index] as u32;
    }
    for _ in assigned..255 {
        let winner = remainders
            .iter()
            .enumerate()
            .max_by_key(|(index, remainder)| (**remainder, std::cmp::Reverse(*index)))
            .map(|(index, _)| index)
            .unwrap_or(0);
        encoded[winner] = encoded[winner].saturating_add(1);
        remainders[winner] = 0;
    }
    encoded
}

#[derive(Clone)]
struct SurfaceMipLevel {
    width: u32,
    height: u32,
    indices: Vec<u8>, // 每像素四个小端 u32。
    weights: Vec<u8>, // 每像素四个归一化 u8。
}

fn read_surface_index(bytes: &[u8], texel: usize, slot: usize) -> u32 {
    let offset = texel * 16 + slot * 4;
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("索引槽已校验"))
}

fn select_surface_slots(contributions: BTreeMap<u32, u64>) -> ([u32; 4], [u8; 4]) {
    if contributions.is_empty() {
        // 未涂绘区域在全部Mip中保持空白，不虚构第一层贡献。
        return ([0; 4], [0; 4]); // 零索引与零权重共同表示裸地。
    } // 有贡献的像素继续使用原归一化规则。
    let mut ranked = contributions.into_iter().collect::<Vec<_>>();
    ranked.sort_by(|(left_index, left_weight), (right_index, right_weight)| {
        right_weight
            .cmp(left_weight)
            .then_with(|| left_index.cmp(right_index))
    });
    let mut indices = [0u32; 4];
    let mut values = [0u64; 4];
    for (slot, (index, value)) in ranked.into_iter().take(4).enumerate() {
        indices[slot] = index;
        values[slot] = value;
    }
    (indices, normalize_four(values))
}

fn push_surface_texel(
    indices: &mut Vec<u8>,
    weights: &mut Vec<u8>,
    selected_indices: [u32; 4],
    selected_weights: [u8; 4],
) {
    for index in selected_indices {
        indices.extend_from_slice(&index.to_le_bytes());
    }
    weights.extend_from_slice(&selected_weights);
}

fn sanitize_surface_source(
    index_source: &[u8],
    weight_source: &[u8],
    resolution: [u32; 2],
    layer_count: u32,
) -> Result<SurfaceMipLevel, String> {
    validate_image_source(resolution, index_source.len(), 16)?;
    validate_image_source(resolution, weight_source.len(), 4)?;
    if layer_count == 0 {
        if index_source
            .iter()
            .chain(weight_source)
            .any(|value| *value != 0)
        {
            // 空层只能保存规范零画布，不能隐藏遗留贡献。
            return Err("空地表层表要求索引和权重作者源全部为零".to_owned()); // 失败仍保留磁盘完整旧版本。
        } // 整图检查在已有后台保存任务内执行。
        return Ok(SurfaceMipLevel {
            // 保留原分辨率，让重新落笔继续复用画布。
            width: resolution[0],            // 索引权重共用原宽度。
            height: resolution[1],           // 索引权重共用原高度。
            indices: index_source.to_vec(),  // 空表仍输出合法配对纹理。
            weights: weight_source.to_vec(), // 不填充任何隐藏材质。
        }); // 后续Mip同样保持全零。
    }
    let texel_count = resolution[0] as usize * resolution[1] as usize;
    let mut indices = Vec::with_capacity(index_source.len());
    let mut weights = Vec::with_capacity(weight_source.len());
    for texel in 0..texel_count {
        let mut contributions = BTreeMap::<u32, u64>::new();
        for slot in 0..4 {
            let index = read_surface_index(index_source, texel, slot);
            if index >= layer_count {
                return Err(format!(
                    "地表像素{texel}的槽{slot}引用层{index}，但场景只有{layer_count}层"
                ));
            }
            let value = u64::from(weight_source[texel * 4 + slot]);
            if value > 0 {
                *contributions.entry(index).or_default() += value;
            }
        }
        let (selected_indices, selected_weights) = select_surface_slots(contributions);
        push_surface_texel(
            &mut indices,
            &mut weights,
            selected_indices,
            selected_weights,
        );
    }
    Ok(SurfaceMipLevel {
        width: resolution[0],
        height: resolution[1],
        indices,
        weights,
    })
}

fn downsample_surface(source: &SurfaceMipLevel) -> SurfaceMipLevel {
    let width = (source.width / 2).max(1);
    let height = (source.height / 2).max(1);
    let mut indices = Vec::with_capacity(width as usize * height as usize * 16);
    let mut weights = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..height {
        for x in 0..width {
            let mut contributions = BTreeMap::<u32, u64>::new();
            let source_x_start = x * source.width / width;
            let source_x_end = (x + 1) * source.width / width;
            let source_y_start = y * source.height / height;
            let source_y_end = (y + 1) * source.height / height;
            for source_y in source_y_start..source_y_end {
                for source_x in source_x_start..source_x_end {
                    let texel = (source_y * source.width + source_x) as usize;
                    for slot in 0..4 {
                        let value = u64::from(source.weights[texel * 4 + slot]);
                        if value > 0 {
                            *contributions
                                .entry(read_surface_index(&source.indices, texel, slot))
                                .or_default() += value;
                        }
                    }
                }
            }
            let (selected_indices, selected_weights) = select_surface_slots(contributions);
            push_surface_texel(
                &mut indices,
                &mut weights,
                selected_indices,
                selected_weights,
            );
        }
    }
    SurfaceMipLevel {
        width,
        height,
        indices,
        weights,
    }
}

/// 从作者R8源生成包含完整Mip链的线性KTX2。
pub fn encode_r8(source: &[u8], resolution: [u32; 2]) -> Result<Vec<u8>, String> {
    validate_image_source(resolution, source.len(), 1)?;
    let mut levels = vec![MipLevel {
        width: resolution[0],
        height: resolution[1],
        pixels: source.to_vec(),
    }];
    while levels
        .last()
        .is_some_and(|level| level.width > 1 || level.height > 1)
    {
        let next = downsample_r8(levels.last().expect("Mip链至少含基础级"));
        levels.push(next);
    }
    let dfd = build_dfd(1, 7, &[0], 255, false);
    encode_uncompressed(VK_FORMAT_R8_UNORM, 1, &dfd, &levels, 0)
}

/// 从四槽索引与权重作者源一次生成严格匹配的完整 Mip KTX2。
pub fn encode_surface_maps(
    index_source: &[u8],
    weight_source: &[u8],
    resolution: [u32; 2],
    layer_count: u32,
) -> Result<(Vec<u8>, Vec<u8>), String> {
    let mut surface_levels = vec![sanitize_surface_source(
        index_source,
        weight_source,
        resolution,
        layer_count,
    )?];
    while surface_levels
        .last()
        .is_some_and(|level| level.width > 1 || level.height > 1)
    {
        let next = downsample_surface(surface_levels.last().expect("Mip链至少含基础级"));
        surface_levels.push(next);
    }
    let index_levels = surface_levels
        .iter()
        .map(|level| MipLevel {
            width: level.width,
            height: level.height,
            pixels: level.indices.clone(),
        })
        .collect::<Vec<_>>();
    let weight_levels = surface_levels
        .into_iter()
        .map(|level| MipLevel {
            width: level.width,
            height: level.height,
            pixels: level.weights,
        })
        .collect::<Vec<_>>();
    let index_dfd = build_dfd(16, 31, &[0, 1, 2, 15], u32::MAX, false);
    let weight_dfd = build_dfd(4, 7, &[0, 1, 2, 15], 255, false);
    Ok((
        encode_uncompressed(VK_FORMAT_R32G32B32A32_UINT, 4, &index_dfd, &index_levels, 0)?,
        encode_uncompressed(VK_FORMAT_R8G8B8A8_UNORM, 1, &weight_dfd, &weight_levels, 0)?,
    ))
}

/// 从等尺寸 RGBA8 切片生成完整 Mip 二维数组；每级内数据按切片连续排列。
pub fn encode_rgba8_array(
    slices: &[Vec<u8>],
    resolution: [u32; 2],
    kind: Rgba8ArrayKind,
) -> Result<Vec<u8>, String> {
    if slices.is_empty() {
        return Err("纹理数组至少需要一个切片".to_owned());
    }
    let layer_count = u32::try_from(slices.len())
        .map_err(|_| "纹理数组切片数量超出 KTX2 u32 可表达范围".to_owned())?;
    for slice in slices {
        validate_image_source(resolution, slice.len(), 4)?;
    }
    let mut per_slice = Vec::with_capacity(slices.len());
    for slice in slices {
        let mut levels = vec![MipLevel {
            width: resolution[0],
            height: resolution[1],
            pixels: slice.clone(),
        }];
        while levels
            .last()
            .is_some_and(|level| level.width > 1 || level.height > 1)
        {
            levels.push(downsample_rgba8(
                levels.last().expect("Mip链至少含基础级"),
                kind,
            ));
        }
        per_slice.push(levels);
    }
    let level_count = per_slice[0].len();
    let mut levels = Vec::with_capacity(level_count);
    for mip in 0..level_count {
        let mut pixels = Vec::new();
        for slice in &per_slice {
            pixels.extend_from_slice(&slice[mip].pixels);
        }
        levels.push(MipLevel {
            width: per_slice[0][mip].width,
            height: per_slice[0][mip].height,
            pixels,
        });
    }
    let srgb = kind.is_srgb();
    let dfd = build_dfd(4, 7, &[0, 1, 2, 15], 255, srgb);
    encode_uncompressed(
        if srgb {
            VK_FORMAT_R8G8B8A8_SRGB
        } else {
            VK_FORMAT_R8G8B8A8_UNORM
        },
        1,
        &dfd,
        &levels,
        layer_count,
    )
}

fn srgb_to_linear(value: u8) -> f32 {
    let value = f32::from(value) / 255.0;
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(value: f32) -> u8 {
    let value = value.clamp(0.0, 1.0);
    let encoded = if value <= 0.003_130_8 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    };
    (encoded * 255.0).round().clamp(0.0, 255.0) as u8
}

fn encode_normal_component(value: f32) -> u8 {
    ((value.clamp(-1.0, 1.0) * 0.5 + 0.5) * 255.0)
        .round()
        .clamp(0.0, 255.0) as u8
}

fn downsample_rgba8(source: &MipLevel, kind: Rgba8ArrayKind) -> MipLevel {
    let width = (source.width / 2).max(1);
    let height = (source.height / 2).max(1);
    let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..height {
        for x in 0..width {
            let mut byte_sums = [0u64; 4];
            let mut float_sums = [0.0_f32; 3];
            let mut count = 0u64;
            let source_x_start = x * source.width / width;
            let source_x_end = (x + 1) * source.width / width;
            let source_y_start = y * source.height / height;
            let source_y_end = (y + 1) * source.height / height;
            for source_y in source_y_start..source_y_end {
                for source_x in source_x_start..source_x_end {
                    let offset = ((source_y * source.width + source_x) * 4) as usize;
                    for channel in 0..4 {
                        byte_sums[channel] += u64::from(source.pixels[offset + channel]);
                    }
                    match kind {
                        Rgba8ArrayKind::SrgbColor => {
                            for (channel, sum) in float_sums.iter_mut().enumerate() {
                                *sum += srgb_to_linear(source.pixels[offset + channel]);
                            }
                        }
                        Rgba8ArrayKind::TangentNormal => {
                            for (channel, sum) in float_sums.iter_mut().enumerate() {
                                *sum += f32::from(source.pixels[offset + channel]) / 127.5 - 1.0;
                            }
                        }
                        Rgba8ArrayKind::LinearData => {}
                    }
                    count += 1;
                }
            }
            match kind {
                Rgba8ArrayKind::SrgbColor => {
                    for sum in float_sums {
                        pixels.push(linear_to_srgb(sum / count as f32));
                    }
                }
                Rgba8ArrayKind::LinearData => {
                    for sum in &byte_sums[..3] {
                        pixels.push(((*sum + count / 2) / count) as u8);
                    }
                }
                Rgba8ArrayKind::TangentNormal => {
                    let length = (float_sums[0] * float_sums[0]
                        + float_sums[1] * float_sums[1]
                        + float_sums[2] * float_sums[2])
                        .sqrt();
                    let normal = if length > 0.000_001 {
                        [
                            float_sums[0] / length,
                            float_sums[1] / length,
                            float_sums[2] / length,
                        ]
                    } else {
                        [0.0, 0.0, 1.0]
                    };
                    pixels.extend(normal.map(encode_normal_component));
                }
            }
            pixels.push(((byte_sums[3] + count / 2) / count) as u8);
        }
    }
    MipLevel {
        width,
        height,
        pixels,
    }
}
/// 把高度顶点转换成每单元NW/NE/SW/SE四角的RGBA16_UNORM KTX2。
pub fn encode_height_cells(heights: &[u16], resolution: [u32; 2]) -> Result<Vec<u8>, String> {
    let [width, height] = resolution;
    if width < 2 || height < 2 || heights.len() != width as usize * height as usize {
        return Err("高度单元源尺寸与高度场分辨率不一致".to_owned());
    }
    let mut pixels = Vec::with_capacity((width - 1) as usize * (height - 1) as usize * 8);
    for z in 0..height - 1 {
        for x in 0..width - 1 {
            for value in [
                heights[(z * width + x) as usize],
                heights[(z * width + x + 1) as usize],
                heights[((z + 1) * width + x) as usize],
                heights[((z + 1) * width + x + 1) as usize],
            ] {
                pixels.extend_from_slice(&value.to_le_bytes());
            }
        }
    }
    let levels = [MipLevel {
        width: width - 1,
        height: height - 1,
        pixels,
    }];
    let dfd = build_dfd(8, 15, &[0, 1, 2, 15], u16::MAX as u32, false);
    encode_uncompressed(VK_FORMAT_R16G16B16A16_UNORM, 2, &dfd, &levels, 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::asset::RenderAssetUsages;
    use bevy::image::{CompressedImageFormats, Image, ImageSampler, ImageType};
    use bevy::render::render_resource::TextureFormat;

    #[test] // 清空后保存和重新局部绘制都不能凭空恢复第一层全覆盖。
    fn empty_surface_preserves_zero_sources_and_mips() {
        let empty = sanitize_surface_source(&[0; 64], &[0; 16], [2, 2], 0).unwrap(); // 空表与全零画布合法。
        let mip = downsample_surface(&empty); // 最小Mip仍须没有贡献。
        assert_eq!(mip.weights, [0; 4]); // 禁止归一化制造隐藏第一层。
        assert_eq!(mip.indices, [0; 16]); // 空权重同时保留规范零索引。
        let (indices, weights) = encode_surface_maps(&[0; 64], &[0; 16], [2, 2], 0).unwrap(); // 实际KTX2配对输出支持空表。
        for payload in [&indices, &weights] {
            // 用正式Bevy加载器回读两张派生图。
            let image = decode_with_bevy(payload); // 覆盖文件格式与完整Mip解码。
            assert_eq!(image.texture_descriptor.mip_level_count, 2); // 原分辨率与完整Mip链都应保留。
            assert!(image.data.as_ref().unwrap().iter().all(|value| *value == 0)); // 保存再读不能复活材质贡献。
        } // 配对文件都须符合空地表约定。
        assert!(sanitize_surface_source(&[0; 16], &[255, 0, 0, 0], [1, 1], 0).is_err()); // 空表不能遗留有效权重。
        let partial =
            sanitize_surface_source(&[0; 32], &[255, 0, 0, 0, 0, 0, 0, 0], [2, 1], 1).unwrap(); // 重新落笔只覆盖其中一格。
        assert_eq!(partial.weights, [255, 0, 0, 0, 0, 0, 0, 0]); // 没画的区域保存后继续为空。
    } // 空地表保存回归结束。

    fn header_u32(payload: &[u8], index: usize) -> u32 {
        let offset = 12 + index * 4;
        u32::from_le_bytes(payload[offset..offset + 4].try_into().unwrap())
    }

    fn level_byte_length(payload: &[u8], level: usize) -> u64 {
        let offset = HEADER_BYTES + level * LEVEL_INDEX_BYTES + 8;
        u64::from_le_bytes(payload[offset..offset + 8].try_into().unwrap())
    }

    #[test]
    fn indexed_surface_mips_keep_paired_container_and_full_chain() {
        let mut indices = Vec::new();
        for _ in 0..16 {
            for index in [0u32, 1, 0, 0] {
                indices.extend_from_slice(&index.to_le_bytes());
            }
        }
        let weights = [200, 55, 0, 0].repeat(16);
        let (index_payload, weight_payload) =
            encode_surface_maps(&indices, &weights, [4, 4], 2).unwrap();
        assert_eq!(&index_payload[..12], KTX2_MAGIC);
        assert_eq!(header_u32(&index_payload, 0), VK_FORMAT_R32G32B32A32_UINT);
        assert_eq!(header_u32(&weight_payload, 0), VK_FORMAT_R8G8B8A8_UNORM);
        assert_eq!(header_u32(&index_payload, 7), 3);
        assert_eq!(header_u32(&weight_payload, 7), 3);
    }

    #[test]
    fn non_power_of_two_sources_keep_gpu_mip_dimensions_and_odd_edges() {
        let source = MipLevel {
            width: 3,
            height: 5,
            pixels: (0..15).collect(),
        };
        let level_one = downsample_r8(&source);
        let level_two = downsample_r8(&level_one);

        assert_eq!((level_one.width, level_one.height), (1, 2));
        assert_eq!((level_two.width, level_two.height), (1, 1));
        assert!(level_one.pixels.last().is_some_and(|value| *value >= 10));

        let payload = encode_r8(&source.pixels, [3, 5]).unwrap();
        assert_eq!(header_u32(&payload, 7), 3);
        assert_eq!(level_byte_length(&payload, 0), 15);
        assert_eq!(level_byte_length(&payload, 1), 2);
        assert_eq!(level_byte_length(&payload, 2), 1);
        let decoded = decode_with_bevy(&payload);
        assert_eq!(decoded.texture_descriptor.mip_level_count, 3);
    }

    #[test]
    fn r8_and_height_cells_use_expected_formats() {
        let density = encode_r8(&[0, 64, 128, 255], [2, 2]).unwrap();
        assert_eq!(header_u32(&density, 0), VK_FORMAT_R8_UNORM);
        let height = encode_height_cells(&[0, 1, 2, 3], [2, 2]).unwrap();
        assert_eq!(header_u32(&height, 0), VK_FORMAT_R16G16B16A16_UNORM);
        assert_eq!(header_u32(&height, 2), 1);
        assert_eq!(header_u32(&height, 3), 1);
    }

    #[test]
    fn four_channel_integer_normalization_is_exact() {
        let normalized = normalize_four([200, 100, 50, 25]);
        assert_eq!(
            normalized.iter().map(|value| *value as u32).sum::<u32>(),
            255
        );
    }

    fn decode_with_bevy(payload: &[u8]) -> Image {
        Image::from_buffer(
            payload,
            ImageType::Extension("ktx2"),
            CompressedImageFormats::NONE,
            header_u32(payload, 0) == VK_FORMAT_R8G8B8A8_SRGB,
            ImageSampler::nearest(),
            RenderAssetUsages::default(),
        )
        .expect("编辑器生成的未压缩KTX2必须能被Bevy正式图片解码器读取")
    }

    #[test]
    fn generated_ktx2_files_decode_through_bevy_runtime_path() {
        let density = decode_with_bevy(&encode_r8(&[0, 64, 128, 255], [2, 2]).unwrap());
        assert_eq!(density.texture_descriptor.format, TextureFormat::R8Unorm);
        assert_eq!(density.texture_descriptor.mip_level_count, 2);
        assert_eq!(density.data.as_deref().unwrap()[..4], [0, 64, 128, 255]);

        let mut surface_indices = Vec::new();
        for _ in 0..16 {
            for index in [0u32, 1, 0, 0] {
                surface_indices.extend_from_slice(&index.to_le_bytes());
            }
        }
        let surface_weights = [200, 55, 0, 0].repeat(16);
        let (index_payload, weight_payload) =
            encode_surface_maps(&surface_indices, &surface_weights, [4, 4], 2).unwrap();
        let indices = decode_with_bevy(&index_payload);
        let weights = decode_with_bevy(&weight_payload);
        assert_eq!(indices.texture_descriptor.format, TextureFormat::Rgba32Uint);
        assert_eq!(weights.texture_descriptor.format, TextureFormat::Rgba8Unorm);
        assert_eq!(indices.texture_descriptor.mip_level_count, 3);
        assert_eq!(weights.texture_descriptor.mip_level_count, 3);

        let height = decode_with_bevy(&encode_height_cells(&[0, 1, 2, 3], [2, 2]).unwrap());
        assert_eq!(height.texture_descriptor.format, TextureFormat::Rgba16Unorm);
        assert_eq!(height.data.as_deref().unwrap().len(), 8);
    }

    #[test]
    fn surface_downsample_aggregates_duplicate_materials_before_top_four_selection() {
        let mut indices = Vec::new();
        for pixel_indices in [
            [7u32, 1, 2, 3],
            [7u32, 4, 5, 6],
            [7u32, 8, 9, 10],
            [11u32, 12, 13, 14],
        ] {
            for index in pixel_indices {
                indices.extend_from_slice(&index.to_le_bytes());
            }
        }
        let weights = [100, 60, 50, 45].repeat(4);
        let base = sanitize_surface_source(&indices, &weights, [2, 2], 15).unwrap();
        let mip = downsample_surface(&base);
        assert_eq!((mip.width, mip.height), (1, 1));
        assert_eq!(read_surface_index(&mip.indices, 0, 0), 7); // 三个像素的同一材质必须先聚合。
        assert_eq!(
            mip.weights
                .iter()
                .map(|value| u32::from(*value))
                .sum::<u32>(),
            255
        );
    }

    #[test]
    fn generated_array_declares_real_layers_and_srgb_format() {
        let payload = encode_rgba8_array(
            &[vec![255; 16], vec![128; 16], vec![64; 16]],
            [2, 2],
            Rgba8ArrayKind::SrgbColor,
        )
        .unwrap();
        assert_eq!(header_u32(&payload, 0), VK_FORMAT_R8G8B8A8_SRGB);
        assert_eq!(header_u32(&payload, 5), 3);
        let image = decode_with_bevy(&payload);
        assert_eq!(
            image.texture_descriptor.format,
            TextureFormat::Rgba8UnormSrgb
        );
        assert_eq!(image.texture_descriptor.size.depth_or_array_layers, 3);
    }

    #[test]
    fn color_mips_average_in_linear_space_and_normal_mips_are_renormalized() {
        let source = MipLevel {
            width: 2,
            height: 2,
            pixels: vec![
                0, 0, 0, 255, 255, 255, 255, 255, 0, 0, 0, 255, 255, 255, 255, 255,
            ],
        };
        let color = downsample_rgba8(&source, Rgba8ArrayKind::SrgbColor);
        assert_eq!(&color.pixels[..3], &[188, 188, 188]); // 线性中灰编码回sRGB约为188，不是字节中值128。

        let opposed = MipLevel {
            width: 2,
            height: 2,
            pixels: vec![
                255, 128, 128, 255, 0, 128, 128, 255, 128, 255, 128, 255, 128, 0, 128, 255,
            ],
        };
        let normal = downsample_rgba8(&opposed, Rgba8ArrayKind::TangentNormal);
        let decoded = [normal.pixels[0], normal.pixels[1], normal.pixels[2]]
            .map(|value| f32::from(value) / 127.5 - 1.0);
        let length = decoded
            .iter()
            .map(|value| value * value)
            .sum::<f32>()
            .sqrt();
        assert!((length - 1.0).abs() < 0.01); // 相消输入也必须输出单位方向，而不是长度近零的字节平均。
        assert!(normal.pixels[2] > 128); // 中性回退方向保持朝向切线空间正Z半球。
    }
}
