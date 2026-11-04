//! 仅后台槽读取有界门文件、独占写入证据；主帧不执行目录遍历或文件等待。
use super::resources::Reply;
use bevy::tasks::IoTaskPool;
use serde_json::Value;
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub(super) fn ordinary(path: &Path) -> Result<fs::Metadata, String> {
    let value = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if value.file_attributes() & 0x400 != 0 {
            return Err("原生预览验收拒绝重解析路径".into());
        }
    }
    if value.file_type().is_symlink() {
        return Err("原生预览验收拒绝链接路径".into());
    }
    Ok(value)
}

pub(super) fn read_now(path: &Path) -> Result<Option<Value>, String> {
    if !path.try_exists().map_err(|error| error.to_string())? {
        return Ok(None);
    }
    let metadata = ordinary(path)?;
    if !metadata.is_file() || metadata.len() > 256 * 1024 {
        return Err("原生输入门必须为256KiB内普通JSON".into());
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|error| error.to_string())?
        .take(256 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > 256 * 1024 {
        return Err("原生输入门读取超过固定预算".into());
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|error| error.to_string())
}

pub(super) fn read(path: PathBuf) -> Reply<Option<Value>> {
    let (sender, receiver) = async_channel::bounded(1);
    IoTaskPool::get()
        .spawn(async move {
            let _ = sender.try_send(read_now(&path));
        })
        .detach();
    receiver
}

pub(super) fn write(path: PathBuf, value: Value) -> Reply<()> {
    let (sender, receiver) = async_channel::bounded(1);
    IoTaskPool::get()
        .spawn(async move {
            let result = (|| {
                let bytes = serde_json::to_vec_pretty(&value).map_err(|error| error.to_string())?;
                let mut file = fs::OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(path)
                    .map_err(|error| error.to_string())?;
                file.write_all(&bytes)
                    .and_then(|_| file.sync_all())
                    .map_err(|error| error.to_string())
            })();
            let _ = sender.try_send(result);
        })
        .detach();
    receiver
}

pub(super) fn poll<T>(reply: &Reply<T>) -> Result<Option<T>, String> {
    match reply.try_recv() {
        Ok(value) => value.map(Some),
        Err(async_channel::TryRecvError::Empty) => Ok(None),
        Err(_) => Err("原生预览验收后台槽意外退出".into()),
    }
}

/// 单飞保存RenderApp原图；尺寸、像素摘要和有界颜色采样均在后台完成。
pub(super) fn image(image: bevy::prelude::Image, path: PathBuf, physical: [u32; 2], view: Value,
    sender: async_channel::Sender<Result<Value, String>>)
{
    IoTaskPool::get().spawn(async move {
        let result = (|| {
            use sha2::{Digest, Sha256};
            let image = image.try_into_dynamic().map_err(|error| error.to_string())?;
            let pixels = image.to_rgba8();
            if u64::from(pixels.width()) * u64::from(pixels.height()) > 16_777_216 {
                return Err("作者GPU图超过1600万像素固定预算".into());
            }
            let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&path)
                .map_err(|error| error.to_string())?;
            image.write_to(&mut file, bevy::image::ImageFormat::Png.as_image_crate_format().unwrap())
                .map_err(|error| error.to_string())?;
            file.sync_all().map_err(|error| error.to_string())?;
            let colors: std::collections::HashSet<_> = pixels.pixels().step_by(19).take(65_536)
                .map(|pixel| pixel.0).collect();
            let rgba: String = Sha256::digest(pixels.as_raw()).iter().map(|byte| format!("{byte:02x}")).collect();
            let matches = [pixels.width(), pixels.height()] == physical;
            Ok(serde_json::json!({"path":path,"physical":[pixels.width(),pixels.height()],"expected_physical":physical,
                "rgba_sha256":rgba,"sampled_unique_colors":colors.len(),"accepted":matches && colors.len()>64,
                "source":"primary-window ScreenshotCaptured from RenderApp","view":view}))
        })();
        let _ = sender.try_send(result);
    }).detach();
}
