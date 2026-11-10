//! 明确JSON路径的只读缺失检查；解析在计算池、文件系统检查在原搜索IO槽，主线程不扫描。

use super::{constants::MAX_PATH_BYTES, references, types::Hit, worker};
use bevy::tasks::AsyncComputeTaskPool;
use std::{
    path::{Component, Path},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

#[cfg(test)]
mod gpu_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
pub(in super::super) use gpu_tests::gpu_assert_missing_assets;

const MAX_PATHS_PER_FILE: usize = 4096;
const MAX_PATH_CHECKS: usize = 16_384;
const MAX_METADATA_CHECKS: usize = 65_536;

enum Stop {
    Cancelled,
    Budget,
}

fn inspect(
    root: &Path,
    value: &str,
    cancel: &AtomicBool,
    metadata_checks: &mut usize,
) -> Result<Option<&'static str>, Stop> {
    let physical = value.split('#').next().unwrap_or("");
    if value.len() > MAX_PATH_BYTES || !references::valid_target(physical) {
        return Ok(Some("路径无效")); // 不猜绝对路径、反斜杠或父目录的另一个写法。
    }
    let mut path = root.to_path_buf();
    let mut parts = Path::new(physical).components().peekable();
    while let Some(Component::Normal(part)) = parts.next() {
        if cancel.load(Ordering::Relaxed) {
            return Err(Stop::Cancelled);
        }
        if *metadata_checks == MAX_METADATA_CHECKS {
            return Err(Stop::Budget);
        }
        path.push(part);
        *metadata_checks += 1;
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) if worker::linked(&metadata) => return Ok(Some("链接路径无法检查")),
            Ok(metadata) if parts.peek().is_some() && !metadata.is_dir() => {
                return Ok(Some("父路径不是目录"));
            }
            Ok(metadata) if parts.peek().is_none() && !metadata.is_file() => {
                return Ok(Some("目标不是文件"));
            }
            Ok(_) => (),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Some("文件缺失"));
            }
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                return Ok(Some("无权访问，无法检查"));
            }
            Err(_) => return Ok(Some("无法检查")),
        }
    }
    Ok(None)
}

pub(super) async fn check_file(
    text: Arc<str>,
    root: &Path,
    maximum: usize,
    checked: &mut usize,
    metadata_checks: &mut usize,
    cancel: Arc<AtomicBool>,
) -> Result<(Vec<Hit>, bool), String> {
    let parsing_cancel = cancel.clone();
    let (paths, mut limited) = AsyncComputeTaskPool::get()
        .spawn(async move { references::collect_json(&text, MAX_PATHS_PER_FILE, &parsing_cancel) })
        .await?; // 完整语法通过后才做元数据检查，坏JSON不产生半份诊断。
    let mut hits = Vec::new();
    for reference in paths {
        if cancel.load(Ordering::Relaxed) {
            return Err("缺失检查已取消".into());
        }
        if *checked == MAX_PATH_CHECKS {
            limited = true;
            break;
        }
        let reason = match inspect(root, &reference.path, &cancel, metadata_checks) {
            Ok(reason) => reason,
            Err(Stop::Budget) => {
                limited = true;
                break;
            }
            Err(Stop::Cancelled) => return Err("缺失检查已取消".into()),
        };
        *checked += 1;
        if let Some(reason) = reason {
            let mut hit = reference.hit;
            // 行池仍有固定片段预算；完整引用由中央选区显示，不保留每份JSON全文。
            let snippet = format!("{reason} · {} · {}", reference.path, hit.snippet);
            hit.snippet = snippet.chars().take(201).collect();
            if snippet.chars().nth(201).is_some() {
                hit.snippet.push('…');
            }
            hits.push(hit);
            if hits.len() == maximum {
                limited = true;
                break;
            }
        }
    }
    Ok((hits, limited))
}
