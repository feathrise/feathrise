//! 地表预览的正式PBR层表快照、配对KTX2全Mip与Image解码。

use super::super::super::{ktx2::encode_surface_maps, resources::SurfacePreviewImage}; // 复用作者保存的配对编码器。
use bevy::asset::RenderAssetUsages; // 生成图片保留CPU与GPU用途。
use bevy::image::{
    CompressedImageFormats, ImageAddressMode, ImageFilterMode, ImageSampler,
    ImageSamplerDescriptor, ImageType,
}; // 与正式KTX2加载保持一致。
use bevy::prelude::*; // 颜色空间转换及GPU参数向量。
use bevy::render::storage::ShaderBuffer; // 任意长度层表CPU编码放在后台。
use feathrise_environment::terrain_surface::TerrainSurfaceIndexedLayer; // 使用正式GPU层布局。
use feathrise_map_content::{MapSurfaceAsset, SurfacePresetRegistry}; // 预设与局部索引权威来源。 // 复用保存时的正式配对Mip编码。

/// 把同修订预设冻结为正式GPU参数；不得退化为纯base_color显示。
pub(super) fn layer_table(
    surface: &MapSurfaceAsset,
    presets: &SurfacePresetRegistry,
) -> Result<(Vec<TerrainSurfaceIndexedLayer>, Vec<f32>), String> {
    // 只在真实修订边界构造。
    u32::try_from(surface.layers.len()).map_err(|_| "地表作者层数超出u32索引".to_owned())?; // 严格保存正式索引边界。
    let mut table = Vec::with_capacity(surface.layers.len().max(1)); // 空表也保留合法GPU哨兵。
    let mut crater_coefficients = Vec::with_capacity(surface.layers.len()); // 复用原有层表冻结循环，不新增一轮层扫描。
    for layer in &surface.layers {
        // 保留作者局部索引顺序，不按预设排序合并。
        let preset = presets
            .resolve(&layer.preset_id)
            .ok_or_else(|| format!("地表层{}的预设{}不存在", layer.id, layer.preset_id))?; // 缺失引用绝不猜测颜色。
        let slice = presets
            .resolved
            .texture_slice(&layer.preset_id)
            .ok_or_else(|| format!("预设{}缺少数组切片", layer.preset_id))?; // 与正式纹理数组切片一致。
        let color = Color::srgba(
            preset.base_color_srgba[0],
            preset.base_color_srgba[1],
            preset.base_color_srgba[2],
            preset.base_color_srgba[3],
        )
        .to_linear(); // GPU混合使用线性颜色。
        crater_coefficients.push(preset.crater_response); // GPU参数与玩法系数来自同一次预设解析。
        table.push(TerrainSurfaceIndexedLayer {
            // 与运行时动态地表完全相同布局与参数。
            base_color: Vec4::new(color.red, color.green, color.blue, color.alpha), // 仅作为纹理乘色。
            pbr_and_texture: Vec4::new(
                preset.perceptual_roughness,
                preset.metallic,
                preset.tile_meters,
                slice as f32,
            ), // 保留粗糙度金属度世界平铺及切片。
            normal_height_projection: Vec4::new(
                preset.normal_strength,
                preset.height_blend_strength,
                preset.projection.code() as f32,
                if preset.normal_y_flip { 1.0 } else { 0.0 },
            ), // 保留法线高度三平面参数。
        }); // 一个作者层独立占一个GPU槽。
    } // 完成有序层快照。
    if table.is_empty() {
        table.push(TerrainSurfaceIndexedLayer {
            // 空作者表不能绑定零长度缓冲。
            base_color: Vec4::ONE, // 零作者层Shader不会读取哨兵。
            pbr_and_texture: Vec4::new(1.0, 0.0, 1.0, 0.0), // 固定合法中性值。
            normal_height_projection: Vec4::ZERO, // 不添加法线或投影效果。
        });
    } // 哨兵不回写作者层。
    Ok((table, crater_coefficients)) // 后台序列化完整层表并构建CPU响应。
}

/// 完整图片编码解码均在single-flight后台任务调用。
#[allow(clippy::too_many_arguments)] // 参数都是同一修订不可变快照。
pub(super) fn build_surface_preview(
    indices: &[u8],                         // 四槽小端u32局部索引。
    weights: &[u8],                         // 四槽线性RGBA权重。
    resolution: [u32; 2],                   // 两张图共同尺寸。
    layer_count: u32,                       // 保留空作者表语义。
    table: Vec<TerrainSurfaceIndexedLayer>, // 与数组切片相同修订的GPU参数。
    array_paths: [String; 3],               // 完整全局三数组路径。
    world_min: Vec2,                        // 正式世界UV最小值。
    world_max: Vec2,                        // 正式世界UV最大值。
) -> Result<SurfacePreviewImage, String> {
    let (index_ktx2, weight_ktx2) = encode_surface_maps(indices, weights, resolution, layer_count)?; // 与保存产物共用去重、归一化和全Mip逻辑。
    let decode = |bytes: &[u8]| {
        // 两张图都必须完整成功才能发布。
        Image::from_buffer(
            bytes,
            ImageType::Extension("ktx2"),
            CompressedImageFormats::NONE,
            false,
            ImageSampler::Descriptor(ImageSamplerDescriptor {
                // 线性数值不能sRGB解码。
                address_mode_u: ImageAddressMode::ClampToEdge, // 世界边界不循环作者遮罩。
                address_mode_v: ImageAddressMode::ClampToEdge, // 与正式权重纹理相同。
                address_mode_w: ImageAddressMode::ClampToEdge, // 单层二维图。
                mag_filter: ImageFilterMode::Linear,           // 正式加载器相同采样设置。
                min_filter: ImageFilterMode::Linear,           // 远近均由正式Shader读取。
                mipmap_filter: ImageFilterMode::Linear,        // 全Mip覆盖远景。
                ..default()                                    // 其他设置保持引擎默认。
            }),
            RenderAssetUsages::default(),
        )
        .map_err(|error| format!("地表预览KTX2解码失败：{error}")) // 解码错误不登记半套资源。
    }; // 数值纹理格式由KTX2正式头确定。
    Ok(SurfacePreviewImage {
        crater_response: None, // 生产单飞任务在同一后台阶段附上经过校验的作者响应。
        // 主世界只接收完整拥有结果。
        indices: decode(&index_ktx2)?,  // RGBA32_UINT完整Mip整数索引。
        weights: decode(&weight_ktx2)?, // RGBA8_UNORM同版权重。
        layer_table: ShaderBuffer::from(table), // CPU存储表序列化也不进入主线程。
        layer_count,
        array_paths,
        world_min,
        world_max, // 参数身份与像素原子发布。
    }) // GPU登记由下一阶段负责。
}

#[cfg(test)] // 不启动GPU即可验证不再生成纯色彩图。
mod tests {
    use super::*; // 测试生产编码路径。
    use bevy::render::render_resource::TextureFormat; // 核对正式数值格式。

    #[test] // 落笔必须保留作者局部索引及强度，不能把预设白色乘色烘焙成白圈。
    fn surface_preview_preserves_painted_local_index_and_empty_neighbors() {
        let mut indices = vec![0; 64]; // 四个纹素只有第一项有贡献。
        indices[..4].copy_from_slice(&1_u32.to_le_bytes()); // 局部第二层而非全局纹理切片。
        let mut weights = vec![0; 16]; // 邻居保持裸地。
        weights[0] = 255; // 单次完整强度落笔。
        let table = vec![
            TerrainSurfaceIndexedLayer {
                // 固定两层足以验证不丢局部索引。
                base_color: Vec4::ONE, // 常见贴图预设的白色仅是乘色。
                pbr_and_texture: Vec4::new(0.73, 0.12, 4.0, 3.0), // 不改变PBR参数与全局切片。
                normal_height_projection: Vec4::new(0.9, 0.4, 1.0, 1.0), // 完整法线、高度、三平面和Y翻转。
            };
            2
        ]; // 作者层独立占槽。
        let preview = build_surface_preview(
            &indices,
            &weights,
            [2, 2],
            2,
            table,
            Default::default(),
            Vec2::splat(-50.0),
            Vec2::splat(50.0),
        )
        .unwrap(); // 生产编码保存未涂绘邻居。
        let data = preview.indices.data.unwrap(); // 验证真实解码的索引图。
        assert_eq!(u32::from_le_bytes(data[..4].try_into().unwrap()), 1); // 不能把第二作者层映射到第一层。
        assert_eq!(&preview.weights.data.unwrap()[..16], weights.as_slice()); // 未落笔区域仍为空地表。
        assert_eq!(preview.world_min, Vec2::splat(-50.0)); // 世界UV不使用图片分辨率替代。
        assert_eq!(preview.layer_count, 2); // 两个作者层没有隐式合并。
        let bevy::render::storage::ShaderBufferData::Initialized { data, buffer_size } = preview.layer_table.data else {
            panic!("预览图层表必须带 CPU 上传数据");
        };
        assert_eq!(data.len(), 96); // 正式48字节PBR层记录包含三个vec4。
        assert_eq!(buffer_size, 96);
    } // 完成实际落笔的数值资产回归。

    #[test] // 空表和单次局部涂绘都保留正式索引、权重及全Mip。
    fn surface_preview_preserves_indexed_pbr_maps_and_mips() {
        let preview = build_surface_preview(
            &[0; 64],
            &[0; 16],
            [2, 2],
            0,
            vec![TerrainSurfaceIndexedLayer {
                // 空地表合法GPU哨兵。
                base_color: Vec4::ONE, // 不生成颜色彩图。
                pbr_and_texture: Vec4::new(1.0, 0.0, 1.0, 0.0), // 安全中性值。
                normal_height_projection: Vec4::ZERO, // 空表没有额外扰动。
            }],
            Default::default(),
            Vec2::ZERO,
            Vec2::ONE,
        )
        .unwrap(); // 与正式保存编码共用路径。
        assert_eq!(
            preview.indices.texture_descriptor.format,
            TextureFormat::Rgba32Uint
        ); // 不能变为RGBA8颜色。
        assert_eq!(
            preview.weights.texture_descriptor.format,
            TextureFormat::Rgba8Unorm
        ); // 权重不是sRGB颜色。
        assert_eq!(preview.indices.texture_descriptor.mip_level_count, 2); // 二乘二含一乘一尾Mip。
        assert_eq!(preview.weights.texture_descriptor.mip_level_count, 2); // 配对层数完全一致。
        assert_eq!(preview.layer_count, 0); // GPU哨兵不能成为作者层。
        assert!(
            preview
                .weights
                .data
                .unwrap()
                .iter()
                .all(|value| *value == 0)
        ); // 裸地没有隐含默认层贡献。
    } // 完成空作者源预览回归。
}
