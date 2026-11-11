//! 积水预览复用正式保存的R8全Mip编码，保证F12远景与保存后运行时一致。
use bevy::asset::RenderAssetUsages; // CPU资产保留GPU上传用途。
use bevy::image::{
    CompressedImageFormats, ImageAddressMode, ImageFilterMode, ImageSampler,
    ImageSamplerDescriptor, ImageType,
};
use bevy::prelude::*; // 完整CPU Image结果。 // 线性数据纹理和夹边采样。
pub(super) fn build(source: &[u8], resolution: [u32; 2]) -> Result<Image, String> {
    // 仅在单在途后台任务调用。
    let bytes = super::super::ktx2::encode_r8(source, resolution)?; // 与保存产物共用同一Mip过滤算法。
    Image::from_buffer(
        // 图像解码与全Mip分配均在后台完成。
        &bytes,                       // 完整本次R8 KTX2字节。
        ImageType::Extension("ktx2"), // 使用Bevy正式KTX2解码器。
        CompressedImageFormats::NONE, // R8未压缩，不依赖显卡压缩格式。
        false,                        // 潜力是线性数据，禁止sRGB转换。
        ImageSampler::Descriptor(ImageSamplerDescriptor {
            // 世界端点不循环采样。
            address_mode_u: ImageAddressMode::ClampToEdge, // X轴边缘固定。
            address_mode_v: ImageAddressMode::ClampToEdge, // Z轴边缘固定。
            address_mode_w: ImageAddressMode::ClampToEdge, // 单层二维纹理。
            mag_filter: ImageFilterMode::Linear,           // 近处潜力平滑插值。
            min_filter: ImageFilterMode::Linear,           // 远景缩小保持连续覆盖。
            mipmap_filter: ImageFilterMode::Linear,        // 与正式KTX2相同的跨Mip混合。
            ..default()                                    // 其他采样选项沿用引擎默认。
        }), // 完整线性采样器。
        RenderAssetUsages::default(), // 主世界持有CPU结果并交给GPU上传。
    )
    .map_err(|error| format!("积水预览KTX2解码失败：{error}")) // 失败保留前一完整图像与作者草稿。
} // 结束同源预览构建。
