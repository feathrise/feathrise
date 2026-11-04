//! 线框验收独立重读实际PNG；协议参数和灰背景不能代替正式基本体的可见边。
use bevy::{
    asset::RenderAssetUsages,
    image::{CompressedImageFormats, ImageSampler, ImageType},
    prelude::Image,
};
use serde_json::Value;
use std::{fs, io::Read, path::Path};

/// 仅在后台取证任务中调用；图像预算固定，四条边共28个投影采样不随工程规模增长。
pub(super) fn verify(capture: &Value, path: &Path) -> Result<(), String> {
    if capture["author_view"]["requested"]["shading"][0].as_f64() != Some(0.) {
        return Ok(());
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|error| error.to_string())?
        .take(64 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > 64 * 1024 * 1024 || bytes.len() < 24 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" {
        return Err("线框PNG缺失或超过预算".into());
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
    let height = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 16 * 1024 * 1024 {
        return Err("线框截图尺寸超出验收预算".into());
    }
    let image = Image::from_buffer(
        &bytes,
        ImageType::Extension("png"),
        CompressedImageFormats::NONE,
        true,
        ImageSampler::nearest(),
        RenderAssetUsages::default(),
    )
    .map_err(|error| error.to_string())?
    .try_into_dynamic()
    .map_err(|error| error.to_string())?
    .to_rgba8();
    verify_samples(capture, image.as_raw(), image.width(), image.height())
}

fn verify_samples(capture: &Value, pixels: &[u8], width: u32, height: u32) -> Result<(), String> {
    let proof = &capture["wire_geometry"];
    if proof["source"] != "projected-real-mesh-edges"
        || proof["required_edges"] != 4
        || capture["capture"]["width"] != width
        || capture["capture"]["height"] != height
    {
        return Err("线框缺少真实网格投影身份或像素尺寸不符".into());
    }
    let object = proof["object_id"].as_str().ok_or("线框缺少正式对象ID")?;
    let placements = capture["placements"].as_array().ok_or("线框缺少实际实例")?;
    if placements
        .iter()
        .filter(|item| item["id"] == object && item["asset_target"]["kind"] == "cuboid")
        .count()
        != 1
    {
        return Err("线框投影没有对应唯一正式基本体".into());
    }
    if pixels.len() != width as usize * height as usize * 4 {
        return Err("线框像素缓冲长度不符".into());
    }
    let edges = proof["edges"]
        .as_array()
        .filter(|edges| edges.len() == 4)
        .ok_or("线框必须包含四条真实顶边")?;
    let mut matched_edges = 0;
    for (edge_index, edge) in edges.iter().enumerate() {
        let samples = edge["samples"]
            .as_array()
            .filter(|samples| samples.len() == 7)
            .ok_or("线框每边必须包含七个投影采样")?;
        let mut matched_count = 0;
        let mut previous = None;
        for sample in samples {
            let position = sample["position"]
                .as_array()
                .filter(|point| point.len() == 2)
                .ok_or("线框投影坐标缺失")?;
            let (x, y) = (
                position[0].as_f64().ok_or("线框X坐标无效")?,
                position[1].as_f64().ok_or("线框Y坐标无效")?,
            );
            if !x.is_finite()
                || !y.is_finite()
                || x < 0.
                || y < 0.
                || x >= f64::from(width)
                || y >= f64::from(height)
            {
                return Err("线框投影超出实际图像".into());
            }
            if previous.is_some_and(|(px, py): (f64, f64)| (x - px).hypot(y - py) < 1.) {
                return Err("线框采样重复，不能以同一像素冒充整条边".into());
            }
            previous = Some((x, y));
            let (x, y) = (x.round() as i32, y.round() as i32);
            let matched = (-2..=2).any(|dy| {
                (-2..=2).any(|dx| {
                    let (x, y) = (x + dx, y + dy);
                    if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
                        return false;
                    }
                    let index = (y as usize * width as usize + x as usize) * 4;
                    pixels[index] <= 28 && pixels[index + 1] <= 28 && pixels[index + 2] <= 28
                })
            });
            if sample["matched"] != matched {
                return Err("线框回执与独立PNG像素不符".into());
            }
            matched_count += usize::from(matched);
        }
        if edge["matched"] != matched_count {
            return Err("线框边命中计数与实际像素不符".into());
        }
        if matched_count < 5 {
            return Err(format!(
                "正式物体顶边{edge_index}只有{matched_count}/7处真实线条，不能由网格背景冒充"
            ));
        }
        matched_edges += 1;
    }
    if proof["matched_edges"] != matched_edges {
        return Err("线框完整边数与实际像素不符".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn wire_geometry_rejects_clear_background_forged_pixels_and_repeated_samples() {
        let mut pixels = vec![64; 64 * 64 * 4];
        let edges = (0..4)
            .map(|edge| {
                json!({"matched":7,"samples":(0..7).map(|sample| {
            let (x,y)=(5+sample*7,5+edge*15);let index=(y*64+x)*4;
            pixels[index..index+3].fill(0);
            json!({"position":[x,y],"matched":true})
        }).collect::<Vec<_>>()})
            })
            .collect::<Vec<_>>();
        let capture = json!({"capture":{"width":64,"height":64},
            "placements":[{"id":"cube","asset_target":{"kind":"cuboid"}}],
            "wire_geometry":{"source":"projected-real-mesh-edges","object_id":"cube",
                "required_edges":4,"matched_edges":4,"edges":edges}});
        assert!(verify_samples(&capture, &pixels, 64, 64).is_ok());
        assert!(
            verify_samples(&capture, &vec![64; 64 * 64 * 4], 64, 64).is_err(),
            "灰色空图不能只凭回执通过"
        );
        let mut repeated = capture.clone();
        repeated["wire_geometry"]["edges"][0]["samples"][1] =
            repeated["wire_geometry"]["edges"][0]["samples"][0].clone();
        assert!(verify_samples(&repeated, &pixels, 64, 64).is_err());
        let mut missing = capture;
        missing["placements"][0]["id"] = json!("different-generation");
        assert!(verify_samples(&missing, &pixels, 64, 64).is_err());
    }
}
