//! 进程退出后独立解码私有观察器的本轮GPU原图，不把原生门数量或截图请求当成显示成功。
use super::{ensure, io};
use bevy::{
    asset::RenderAssetUsages,
    image::{CompressedImageFormats, ImageSampler, ImageType},
    prelude::Image,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub(super) fn verify(output: &Path, report: &Value) -> Result<Vec<Value>, String> {
    let root = output
        .join("runtime")
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let mut proofs = Vec::new();
    let editor = &report["editor_gpu_capture"];
    let editor_path = output.join("editor-during-native-preview.png");
    let metadata = io::ordinary(&editor_path)?;
    ensure(metadata.is_file() && metadata.len() <= 128 * 1024 * 1024
        && editor["accepted"] == true && editor["view"]["game"]["playing"] == true
        && editor["source"] == "primary-window ScreenshotCaptured from RenderApp",
        "缺少预览运行时的本轮原作者GPU原图")?;
    let bytes = fs::read(&editor_path).map_err(|error| error.to_string())?;
    let pixels = Image::from_buffer(&bytes, ImageType::Extension("png"), CompressedImageFormats::NONE,
        true, ImageSampler::nearest(), RenderAssetUsages::default())
        .map_err(|error| error.to_string())?.try_into_dynamic()
        .map_err(|error| error.to_string())?.to_rgba8();
    ensure(editor["rgba_sha256"] == hash(pixels.as_raw())
        && editor["physical"] == json!([pixels.width(), pixels.height()]),
        "原作者GPU图实际像素或尺寸与RenderApp回执不符")?;
    proofs.push(json!({"stage":"editor-during-native-preview","path":editor_path,
        "png_file_sha256":hash(&bytes),"decoded_rgba_sha256":hash(pixels.as_raw())}));
    for (index, (stage, mode)) in [
        ("initial-editing", "editing"),
        ("third-person-rotated", "playing"),
        ("original-ui-fullscreen", "playing"),
        ("editing-after-native-x", "editing"),
        ("editing-after-hud", "editing"),
        ("playing-after-cancel", "playing"),
        ("editing-after-alt-f4", "editing"),
        ("playing-after-alt-f4", "playing"),
        ("editing-final", "editing"),
    ]
    .into_iter()
    .enumerate()
    {
        let sequence = index + 1;
        let value = io::read_now(&root.join(format!("capture-{sequence}.json")))?
            .ok_or("原生专项缺少实际GPU回执")?;
        ensure(
            value["schema"] == 1
                && value["token"] == report["token"]
                && value["pid"] == report["game_pid"]
                && value["world_id"] == report["world_id"]
                && value["mode"] == mode
                && value["frame"].as_u64().unwrap_or(0) > 0
                && value["request"]["token"] == report["token"]
                && value["request"]["sequence"] == sequence
                && value["request"]["stage"] == stage
                && value["capture"]["source"] == "Bevy ScreenshotCaptured",
            "GPU回执不是本轮同PID/World及对应阶段",
        )?;
        let path = root.join(format!("capture-{sequence}.png"));
        let metadata = io::ordinary(&path)?;
        ensure(
            metadata.is_file() && metadata.len() <= 128 * 1024 * 1024,
            "GPU原图不是128MiB以内普通文件",
        )?;
        let bytes = fs::read(&path).map_err(|error| error.to_string())?;
        ensure(
            bytes.len() >= 24 && &bytes[..8] == b"\x89PNG\r\n\x1a\n",
            "GPU原图缺少完整PNG签名",
        )?;
        let width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
        let height = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
        ensure(
            width > 0 && height > 0 && u64::from(width) * u64::from(height) <= 16_777_216,
            "GPU原图超出1600万像素固定预算",
        )?;
        let png_hash = hash(&bytes);
        ensure(
            value["capture"]["png_sha256"] == png_hash,
            "GPU原始PNG已被替换",
        )?;
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
        let rgba_hash = hash(image.as_raw());
        ensure(
            value["capture"]["rgba_sha256"] == rgba_hash
                && value["capture"]["width"] == image.width()
                && value["capture"]["height"] == image.height(),
            "独立解码的GPU像素/尺寸与原回执不符",
        )?;
        if mode == "playing" {
            ensure(
                value["control_mode"] == "ThirdPerson"
                    && value["player"]["builtin_capsule"] == true
                    && value["camera"]["active"] == true,
                "Playing原图不具备真实第三人称角色/相机身份",
            )?;
        }
        proofs.push(
            json!({"stage":stage,"path":path,"frame":value["frame"],"mode":mode,
            "png_file_sha256":png_hash,"decoded_rgba_sha256":rgba_hash,"physical":[width,height]}),
        );
    }
    Ok(proofs)
}
