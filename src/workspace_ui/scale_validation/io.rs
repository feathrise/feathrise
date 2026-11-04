//! 本轮自有配置的有界读取和GPU/JSON落盘；主帧只非阻塞轮询回执。
use super::{resources::Reply, types::FileStamp};
use bevy::{prelude::*, tasks::IoTaskPool};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::HashSet, fs::{self, File, OpenOptions}, io::{Read, Write}, path::{Path, PathBuf}, time::UNIX_EPOCH};

pub(super) fn ordinary(path: &Path) -> Result<fs::Metadata, String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| format!("{}：{error}", path.display()))?;
    if metadata.file_type().is_symlink() { return Err("专项路径不能是符号链接".into()); }
    #[cfg(windows)] {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 { return Err("专项路径不能是reparse/junction".into()); }
    }
    Ok(metadata)
}

pub(super) fn read_now(path: &Path) -> Result<Option<FileStamp>, String> {
    if !path.try_exists().map_err(|error| error.to_string())? { return Ok(None); }
    let metadata = ordinary(path)?;
    if !metadata.is_file() || metadata.len() > 256 * 1024 { return Err("隔离作者配置不是256KiB以内的普通文件".into()); }
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    (&mut file).take(256 * 1024 + 1).read_to_end(&mut bytes).map_err(|error| error.to_string())?;
    if bytes.len() > 256 * 1024 { return Err("作者配置读取超过固定预算".into()); }
    let modified = file.metadata().and_then(|info| info.modified()).map_err(|error| error.to_string())?;
    let sha256 = Sha256::digest(&bytes).iter().map(|byte| format!("{byte:02x}")).collect();
    Ok(Some(FileStamp { bytes, modified, sha256 }))
}

pub(super) fn read(path: PathBuf) -> Reply<Option<FileStamp>> {
    let (sender, receiver) = async_channel::bounded(1);
    IoTaskPool::get().spawn(async move { let _ = sender.try_send(read_now(&path)); }).detach();
    receiver
}

pub(super) fn stamp(value: &Option<FileStamp>) -> Value {
    value.as_ref().map_or(json!({"exists":false}), |value| json!({"exists":true,"bytes":value.bytes.len(),
        "sha256":value.sha256,"modified_ns":value.modified.duration_since(UNIX_EPOCH).map(|duration| duration.as_nanos().min(u64::MAX as u128) as u64).ok()}))
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path).map_err(|error| error.to_string())?;
    file.write_all(bytes).and_then(|_| file.sync_all()).map_err(|error| error.to_string())
}

pub(super) fn report(path: PathBuf, value: Value) -> Reply<()> {
    let (sender, receiver) = async_channel::bounded(1);
    IoTaskPool::get().spawn(async move {
        let result = serde_json::to_vec_pretty(&value).map_err(|error| error.to_string()).and_then(|bytes| write_new(&path, &bytes));
        let _ = sender.try_send(result);
    }).detach();
    receiver
}

pub(super) fn image(image: Image, path: PathBuf, physical: [u32; 2], view: Value, sender: async_channel::Sender<Result<Value, String>>) {
    IoTaskPool::get().spawn(async move {
        let result = (|| {
            let image = image.try_into_dynamic().map_err(|error| error.to_string())?;
            // 首个RenderApp回执可能仍是冷管线背景。先保留每张真实原图，拒绝帧也不能丢失现场。
            let mut file = OpenOptions::new().write(true).create_new(true).open(&path).map_err(|error| error.to_string())?;
            image.write_to(&mut file, bevy::image::ImageFormat::Png.as_image_crate_format().unwrap()).map_err(|error| error.to_string())?;
            file.sync_all().map_err(|error| error.to_string())?;
            let pixels = image.to_rgba8();
            let colors: HashSet<_> = pixels.pixels().map(|pixel| pixel.0).collect();
            let dimensions_match = [pixels.width(), pixels.height()] == physical;
            let ui_pixels_ready = colors.len() > 64;
            Ok(json!({"path":path,"physical":[pixels.width(),pixels.height()],"expected_physical":physical,
                "unique_colors":colors.len(),"dimensions_match":dimensions_match,"ui_pixels_ready":ui_pixels_ready,
                "accepted":dimensions_match && ui_pixels_ready,"source":"primary-window ScreenshotCaptured from RenderApp","view":view}))
        })();
        let _ = sender.try_send(result);
    }).detach();
}
