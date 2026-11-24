//! 工程图标的后台有界读取与真实 PNG/JPEG 解码。项目文件保留原字节，面板仅登记缩略图。

use bevy::{asset::RenderAssetUsages, prelude::Image};
use image::{DynamicImage, ImageFormat, ImageReader, Limits};
use std::{fs::File, io::{Cursor, Read}, path::Path};

pub struct DecodedProjectIcon {
    pub bytes: Vec<u8>,
    pub extension: &'static str,
    image: DynamicImage,
}

impl DecodedProjectIcon {
    pub fn preview(self) -> Image {
        // 工程设置面板只显示小图，使用 256 像素缩略图，不把来源图尺寸传给 GPU。
        const PREVIEW_EDGE: u32 = 256;
        let image = if self.image.width().max(self.image.height()) > PREVIEW_EDGE {
            self.image.thumbnail(PREVIEW_EDGE, PREVIEW_EDGE)
        } else {
            self.image
        };
        Image::from_dynamic(image, true, RenderAssetUsages::default())
    }
}

pub fn decode(path: &Path) -> Result<DecodedProjectIcon, String> {
    let metadata = std::fs::metadata(path).map_err(|error| format!("无法访问图标：{error}"))?;
    if !metadata.is_file() {
        return Err("工程图标必须是普通文件".into());
    }
    // 原始字节和解码数据共同使用图像库的标准分配预算，不以固定作者分辨率拒绝有效图像。
    let mut limits = Limits::default();
    let allocation = limits.max_alloc.ok_or("图像解码器未提供分配预算")?;
    if metadata.len() >= allocation {
        return Err("图标文件超出图像解码的内存预算".into());
    }
    let mut bytes = Vec::new();
    File::open(path).and_then(|file| file.take(allocation).read_to_end(&mut bytes))
        .map_err(|error| format!("无法读取图标：{error}"))?;
    let remaining = allocation.saturating_sub(bytes.len() as u64);
    if remaining == 0 {
        return Err("图标文件超出图像解码的内存预算".into());
    }
    let mut reader = ImageReader::new(Cursor::new(&bytes)).with_guessed_format()
        .map_err(|error| format!("无法识别图标：{error}"))?;
    let extension = match reader.format() {
        Some(ImageFormat::Png) => "png",
        Some(ImageFormat::Jpeg) => "jpg",
        _ => return Err("工程图标应为 PNG 或 JPEG 图片".into()),
    };
    limits.max_alloc = Some(remaining);
    reader.limits(limits);
    let image = reader.decode().map_err(|error| format!("无法解码工程图标：{error}"))?;
    Ok(DecodedProjectIcon { bytes, extension, image })
}
