//! 专项证据的非阻塞 GPU 截图和自有 GLB 原子替换。

use super::{MOTIONS, STATIC};
use bevy::{
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
    tasks::IoTaskPool,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Cursor, Read, Write},
    path::Path,
    sync::{Mutex, mpsc},
};

pub(super) type CaptureTask = Mutex<mpsc::Receiver<Result<Option<Value>, String>>>;

pub(super) fn capture(
    world: &mut World,
    pending: &mut Option<CaptureTask>,
    path: &Path,
) -> Result<Option<Value>, String> {
    if let Some(receiver) = pending {
        return match receiver.get_mut().map_err(|error| error.to_string())?.try_recv() {
            Ok(result) => { *pending = None; result }
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(mpsc::TryRecvError::Disconnected) => Err("窗口截图没有返回 GPU 回执".into()),
        };
    }
    let (sender, receiver) = mpsc::sync_channel(1);
    *pending = Some(Mutex::new(receiver));
    let path = path.to_owned();
    world.spawn(Screenshot::primary_window()).observe(move |event: On<ScreenshotCaptured>| {
        let (image, path, sender) = (event.image.clone(), path.clone(), sender.clone());
        IoTaskPool::get().spawn(async move {
            let result = (|| {
                let image = image.try_into_dynamic().map_err(|error| error.to_string())?.to_rgba8();
                let (width, height) = image.dimensions();
                if width < 320 || height < 240 {
                    return Err("GPU 截图没有实际编辑器窗口尺寸".into());
                }
                let clear = image.get_pixel(0, 0);
                let varied = (0..height).step_by(13).flat_map(|y| (0..width).step_by(13).map(move |x| (x, y)))
                    .filter(|&(x, y)| image.get_pixel(x, y) != clear).take(64).count();
                if varied < 64 { return Ok(None); } // 若窗口只交付了清屏帧，下一轮重新请求回读。
                let mut output = Cursor::new(Vec::new());
                image::DynamicImage::ImageRgba8(image).write_to(&mut output, image::ImageFormat::Png)
                    .map_err(|error| error.to_string())?;
                let bytes = output.into_inner();
                write_new(&path, &bytes)?;
                Ok(Some(json!({
                    "source":"Bevy ScreenshotCaptured","width":width,"height":height,
                    "sha256":Sha256::digest(&bytes).iter().map(|byte| format!("{byte:02x}")).collect::<String>(),
                })))
            })();
            let _ = sender.send(result);
        }).detach();
    });
    Ok(None)
}

pub(super) fn replace_clip_file(work: &Path, assets: &Path, original: &[u8]) -> Result<(), String> {
    let project = work.join("project");
    let target = assets.join(MOTIONS);
    let replacement = assets.join(STATIC);
    // 只允许在本轮拥有的真实工程替换原文件，若输入、目录或归属发生变化就保留现场。
    for path in [work, &project, assets, &target, &replacement] {
        if path.canonicalize().ok().as_deref() != Some(path) {
            return Err(format!("拒绝替换越界的验收夹具：{}", path.display()));
        }
    }
    let owner: Value = serde_json::from_slice(&read_small(&work.join(".project-browser-owner.json"))?)
        .map_err(|error| error.to_string())?;
    if assets != project.join("assets")
        || owner["purpose"] != "owned-project-browser-validation"
        || owner["project"] != json!(project)
        || owner["work"] != json!(work)
        || read_small(&target)? != original
    {
        return Err("验收 GLB 原始字节或工程归属已变化，拒绝替换".into());
    }
    let staged = work.join("motion-replacement.part");
    write_new(&staged, &read_small(&replacement)?)?;
    if read_small(&target)? != original {
        return Err("待替换验收 GLB 在准备期间发生变化".into());
    }
    fs::rename(&staged, &target).map_err(|error| format!("自有动作 GLB 原子替换失败：{error}"))
}

fn read_small(path: &Path) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    File::open(path).and_then(|file| file.take(64 * 1024 + 1).read_to_end(&mut bytes))
        .map_err(|error| format!("无法读取自有夹具 {}：{error}", path.display()))?;
    if bytes.len() > 64 * 1024 { return Err("自有验收数据超出夹具预算".into()); }
    Ok(bytes)
}

pub(super) fn report(path: &Path, value: &Value) -> Result<(), String> {
    write_new(path, &serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?)
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    OpenOptions::new().write(true).create_new(true).open(path)
        .and_then(|mut file| file.write_all(bytes))
        .map_err(|error| format!("无法独占写入验收证据 {}：{error}", path.display()))
}
