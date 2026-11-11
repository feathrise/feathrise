//! 项目Rust源码的只读遍历与串行后台解析；不进入target、Git、资源目录或符号链接。

use super::super::{constants::*, types::*, worker::linked};
use super::source_scanner::match_rust;
use crate::ui::external_files::{checked_project, is_project_source, read_project_source};
use bevy::tasks::AsyncComputeTaskPool;
use std::sync::atomic::Ordering;

pub(in super::super) async fn scan_sources(
    request: &SearchRequest,
    result: &mut SearchResult,
    entries: &mut usize,
    total: &mut usize,
) {
    let Ok(project) = checked_project(&request.root) else {
        return;
    }; // 无源码的发行包仍可查JSON引用。
    let Ok(root) = std::fs::read_dir(&project) else {
        result.skipped += 1;
        return;
    };
    let mut stack = vec![root];
    while !stack.is_empty() && !result.limited && !request.cancel.load(Ordering::Relaxed) {
        let Some(entry) = stack.last_mut().unwrap().next() else {
            stack.pop();
            continue;
        };
        let Ok(entry) = entry else {
            result.skipped += 1;
            continue;
        };
        let path = entry.path();
        let Some(relative) = path
            .strip_prefix(&project)
            .ok()
            .and_then(|path| path.to_str())
        else {
            result.skipped += 1;
            continue;
        };
        let relative = relative.replace('\\', "/");
        if relative != "build.rs"
            && !["src", "crates", "apps"]
                .iter()
                .any(|root| relative == *root || relative.starts_with(&format!("{root}/")))
        {
            continue;
        }
        if relative.split('/').any(|part| {
            matches!(
                part,
                "target" | ".git" | ".local" | "node_modules" | "assets"
            )
        }) {
            continue;
        }
        *entries += 1;
        if *entries > MAX_ENTRIES {
            result.limited = true;
            break;
        }
        if relative.len() > MAX_PATH_BYTES {
            result.skipped += 1;
            continue;
        }
        let Ok(metadata) = std::fs::symlink_metadata(&path) else {
            result.skipped += 1;
            continue;
        };
        if linked(&metadata) {
            result.skipped += 1;
            continue;
        }
        if metadata.is_dir() {
            if stack.len() >= MAX_DEPTH {
                result.skipped += 1;
                result.limited = true;
                break;
            }
            match std::fs::read_dir(&path) {
                Ok(dir) => stack.push(dir),
                Err(_) => result.skipped += 1,
            }
            continue;
        }
        if !metadata.is_file() || !is_project_source(&relative) {
            continue;
        }
        let Ok((digest, text)) = read_project_source(&project, &relative) else {
            result.skipped += 1;
            continue;
        };
        result.scanned += 1;
        let target = request.query.clone();
        let cancel = request.cancel.clone();
        let maximum = MAX_HITS - *total;
        let hits = AsyncComputeTaskPool::get()
            .spawn(async move { match_rust(&text, &target, maximum, &cancel) })
            .await;
        let Ok(hits) = hits else {
            result.skipped += 1;
            continue;
        };
        if !hits.is_empty() {
            *total += hits.len();
            result.hits = *total;
            result.files.push(FileHits {
                path: relative,
                identity: ContentIdentity::ProjectSource(digest),
                dirty: false,
                hits,
            });
        }
        if *total == MAX_HITS {
            result.limited = true;
        }
    }
}
