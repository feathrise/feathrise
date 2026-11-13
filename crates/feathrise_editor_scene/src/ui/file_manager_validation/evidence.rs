//! 文件桌面专项的真实GPU回读；截图落盘与字节摘要在IO池完成，不阻塞作者帧。

use bevy::{
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
    tasks::IoTaskPool,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs::OpenOptions, io::{Cursor, Write}, path::Path, sync::{Mutex, mpsc}};

pub(super) type CaptureTask = Mutex<mpsc::Receiver<Result<Option<Value>, String>>>;
pub(super) type DiskTask = Mutex<mpsc::Receiver<Result<Value, String>>>;

pub(super) fn new_files(pending: &mut Option<DiskTask>, directory: &Path) -> Result<Option<Value>, String> {
    if let Some(receiver) = pending {
        return match receiver.get_mut().map_err(|error| error.to_string())?.try_recv() {
            Ok(result) => { *pending = None; result.map(Some) }
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(mpsc::TryRecvError::Disconnected) => Err("新建文件后台磁盘核验没有返回".into()),
        };
    }
    let (sender, receiver) = mpsc::sync_channel(1);
    *pending = Some(Mutex::new(receiver));
    let directory = directory.to_owned();
    IoTaskPool::get().spawn(async move {
        let result = (|| {
            let mut files = serde_json::Map::new();
            for name in ["中文笔记.txt", "中文说明.md"] {
                let bytes = std::fs::read(directory.join(name)).map_err(|error| format!("回读新建文件 {name}：{error}"))?;
                std::str::from_utf8(&bytes).map_err(|error| format!("新建文件不是UTF-8：{error}"))?;
                if !bytes.is_empty() { return Err(format!("新建文件 {name} 不是空文档")); }
                files.insert(name.into(), json!({"bytes":bytes.len(),"utf8":true,"sha256":digest(&bytes)}));
            }
            let bytes = std::fs::read(directory.join("中文空场景.glb")).map_err(|error| format!("回读新建GLB：{error}"))?;
            // 直接用正式 glTF 解析器读取落盘字节，不能只凭扩展名或任务成功回执认定 GLB 有效。
            let glb = gltf::Gltf::from_slice(&bytes).map_err(|error| format!("新建GLB不能由glTF解析器读取：{error}"))?;
            if glb.document.scenes().count() != 1 || glb.document.default_scene().is_none()
                || glb.document.default_scene().is_some_and(|scene| scene.nodes().next().is_some())
                || glb.document.nodes().next().is_some() || glb.document.meshes().next().is_some()
                || glb.document.skins().next().is_some() || glb.document.animations().next().is_some()
            {
                return Err("新建GLB不是单个默认空场景".into());
            }
            files.insert("中文空场景.glb".into(), json!({"bytes":bytes.len(),"sha256":digest(&bytes),
                "parser":"gltf::Gltf::from_slice","default_scene":0,"scenes":1,"nodes":0,"meshes":0,"skins":0,"animations":0}));
            Ok(json!({"source":"后台独立回读实际落盘文件","directory":directory,"files":files}))
        })();
        let _ = sender.send(result);
    }).detach();
    Ok(None)
}

fn digest(bytes: &[u8]) -> String { Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect() }

pub(super) fn capture(world: &mut World, pending: &mut Option<CaptureTask>, path: &Path) -> Result<Option<Value>, String> {
    if let Some(receiver) = pending {
        return match receiver.get_mut().map_err(|error| error.to_string())?.try_recv() {
            Ok(result) => { *pending = None; result }
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(mpsc::TryRecvError::Disconnected) => Err("文件桌面截图未返回GPU回执".into()),
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
                if width < 640 || height < 480 { return Err("文件桌面没有实际工作台截图尺寸".into()); }
                let clear = image.get_pixel(0, 0);
                let varied = (0..height).step_by(13).flat_map(|y| (0..width).step_by(13).map(move |x| (x, y)))
                    .filter(|&(x, y)| image.get_pixel(x, y) != clear).take(64).count();
                if varied < 64 { return Ok(None); }
                let mut output = Cursor::new(Vec::new());
                image::DynamicImage::ImageRgba8(image).write_to(&mut output, image::ImageFormat::Png)
                    .map_err(|error| error.to_string())?;
                let bytes = output.into_inner();
                write_new(&path, &bytes)?;
                Ok(Some(json!({"source":"Bevy ScreenshotCaptured","width":width,"height":height,
                    "sha256":Sha256::digest(&bytes).iter().map(|byte| format!("{byte:02x}")).collect::<String>()})))
            })();
            let _ = sender.send(result);
        }).detach();
    });
    Ok(None)
}

pub(super) fn report(path: &Path, value: &Value) -> Result<(), String> {
    write_new(path, &serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?)
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    OpenOptions::new().write(true).create_new(true).open(path)
        .and_then(|mut file| file.write_all(bytes))
        .map_err(|error| format!("无法独占写入文件桌面证据 {}：{error}", path.display()))
}
