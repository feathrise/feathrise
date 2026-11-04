//! 唯一IO槽受检读取；解码交给计算池，取消任务真正结束前仍占槽。

use super::{decode, source::PcmData};
use bevy::tasks::AsyncComputeTaskPool;
use std::{
    io::Read,
    path::{Component, Path},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

const MAX_FILE_BYTES: usize = 64 * 1024 * 1024;

fn read(root: &Path, relative: &str, cancel: &AtomicBool) -> Result<Vec<u8>, String> {
    if relative.is_empty()
        || relative.contains(['\\', ':', '#'])
        || Path::new(relative)
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err("只能试听assets内的音频文件相对路径".into());
    }
    let root = root
        .canonicalize()
        .map_err(|error| format!("无法打开assets目录：{error}"))?;
    let path = root
        .join(relative)
        .canonicalize()
        .map_err(|error| format!("音频文件无法打开：{error}"))?;
    if !path.starts_with(&root) {
        return Err("音频路径超出assets目录".into());
    }
    let mut file = std::fs::File::open(path).map_err(|error| format!("音频读取失败：{error}"))?;
    let before = file.metadata().map_err(|error| error.to_string())?;
    if !before.is_file() || before.len() > MAX_FILE_BYTES as u64 {
        return Err("音频文件超过64MiB试听预算，或目标不是文件".into());
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(before.len() as usize)
        .map_err(|_| "音频读取内存不足".to_owned())?;
    let mut chunk = [0u8; 64 * 1024];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("音频加载已取消".into());
        }
        let count = file
            .read(&mut chunk)
            .map_err(|error| format!("音频读取失败：{error}"))?;
        if count == 0 {
            break;
        }
        if bytes.len() + count > MAX_FILE_BYTES {
            return Err("音频文件超过64MiB试听预算".into());
        }
        if bytes.capacity() - bytes.len() < count {
            bytes
                .try_reserve_exact(count)
                .map_err(|_| "音频读取内存不足".to_owned())?;
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
    let after = file.metadata().map_err(|error| error.to_string())?;
    if before.len() != after.len()
        || before.modified().ok() != after.modified().ok()
        || bytes.len() as u64 != after.len()
    {
        return Err("音频文件读取时发生变化，请重新载入".into());
    }
    Ok(bytes)
}

pub(super) async fn load(
    root: &Path,
    relative: &str,
    cancel: Arc<AtomicBool>,
) -> Result<Arc<PcmData>, String> {
    let bytes = read(root, relative, &cancel)?;
    let extension = relative
        .rsplit_once('.')
        .map_or("", |(_, ext)| ext)
        .to_ascii_lowercase();
    AsyncComputeTaskPool::get()
        .spawn(async move { decode::decode(bytes, &extension, &cancel) })
        .await
}
