//! 后台受检遍历和串行内容搜索；阻塞读取属于IO池，Unicode匹配属于计算池。

use super::super::{io, navigation, source};
use super::{constants::*, missing_assets, references, types::*};
use crate::asset_catalog::document_format;
use bevy::tasks::AsyncComputeTaskPool;
use std::{
    path::{Component, Path},
    sync::atomic::Ordering,
};

pub(super) fn filters(value: &str) -> Result<Vec<String>, String> {
    let mut tokens = Vec::new();
    for token in value
        .split(|ch: char| ch == ',' || ch == ';' || ch.is_whitespace())
        .filter(|s| !s.is_empty())
    {
        let token = token
            .strip_prefix("*.")
            .or_else(|| token.strip_prefix('.'))
            .unwrap_or(token);
        if token.is_empty()
            || !token
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
        {
            return Err("文件类型使用扩展名，如 json, rs, *.wgsl；无扩展名可填 README".into());
        }
        let token = token.to_ascii_lowercase();
        if !tokens.contains(&token) {
            tokens.push(token);
        }
        if tokens.len() > MAX_FILTERS {
            return Err("一次最多筛选16种文件类型".into());
        }
    }
    Ok(tokens)
}

pub(super) fn accepts(path: &str, filters: &[String]) -> bool {
    if document_format(path).is_none() {
        return false;
    }
    if filters.is_empty() {
        return true;
    }
    let path = Path::new(path);
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    filters
        .iter()
        .any(|token| token == &ext || token == &name || token == name.trim_start_matches('.'))
}

pub(super) fn in_folder(path: &str, folder: &str) -> bool {
    folder.is_empty()
        || path
            .strip_prefix(folder)
            .is_some_and(|tail| tail.starts_with('/'))
}

pub(super) fn linked(metadata: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0 // Junction与其他reparse目录也不能绕过深度/循环边界。
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

/// 只保留匹配与固定长度上下文，不把每个文件的全文带回主世界。
pub(super) fn match_text(
    regex: &regex::Regex,
    source: &str,
    maximum: usize,
    cancel: &std::sync::atomic::AtomicBool,
) -> Vec<Hit> {
    let mut hits = Vec::new();
    let (mut byte, mut line, mut column) = (0, 1, 1);
    for found in regex.find_iter(source).take(maximum) {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        for ch in source[byte..found.start()].chars() {
            if ch == '\n' {
                line += 1;
                column = 1;
            } else {
                column += 1;
            }
        }
        byte = found.start();
        let start = source[..found.start()]
            .char_indices()
            .rev()
            .take_while(|(_, ch)| *ch != '\n')
            .take(24)
            .last()
            .map_or(found.start(), |(byte, _)| byte);
        let left: String = source[start..found.start()]
            .chars()
            .map(|ch| if ch == '\t' { ' ' } else { ch })
            .collect();
        let matched: String = source[found.range()].chars().take(128).collect();
        let right: String = source[found.end()..]
            .chars()
            .take_while(|ch| *ch != '\n')
            .take(48)
            .map(|ch| if ch == '\t' { ' ' } else { ch })
            .collect();
        hits.push(Hit {
            range: found.range(),
            line,
            column,
            snippet: format!("{left}‹{matched}›{right}"),
        });
    }
    hits
}

async fn add_file(
    request: &SearchRequest,
    result: &mut SearchResult,
    regex: &Option<regex::Regex>,
    root: &Path,
    path: String,
    text: std::sync::Arc<str>,
    identity: ContentIdentity,
    dirty: bool,
    total: &mut usize,
    metadata_checks: &mut usize,
) {
    result.scanned += 1;
    let maximum = MAX_HITS - *total;
    let regex = regex.clone();
    let cancel = request.cancel.clone();
    let target = request.query.clone();
    let hits = if request.missing_assets {
        match missing_assets::check_file(text, root, maximum, &mut result.checked_paths, metadata_checks, cancel).await {
            Ok((hits, limited)) => { result.limited |= limited; Ok(hits) }
            Err(error) => Err(error),
        }
    } else { AsyncComputeTaskPool::get()
        .spawn(async move {
            match regex {
                Some(regex) => Ok(match_text(&regex, &text, maximum, &cancel)),
                None => references::match_json(&text, &target, maximum, &cancel),
            }
        })
        .await };
    let Ok(hits) = hits else {
        result.skipped += 1; // 非法/过深JSON整份跳过，原文件和草稿都不变。
        return;
    };
    *total += hits.len();
    result.hits = *total;
    if !hits.is_empty() {
        result.files.push(FileHits {
            path,
            identity,
            dirty,
            hits,
        });
    }
    if *total == MAX_HITS {
        result.limited = true;
    }
}

pub(super) async fn run(
    request: SearchRequest,
    retired: Option<std::sync::Arc<SearchResult>>,
) -> SearchResult {
    drop(retired); // 原显示结果的可变规模析构只发生在后台，并继续占唯一任务槽。
    let mut result = SearchResult {
        ticket: request.ticket,
        references: request.references,
        missing_assets: request.missing_assets,
        root: request.root.clone(),
        folder: request.folder.clone(),
        drafts: request
            .drafts
            .iter()
            .filter(|draft| !source::is_project(&draft.stamp.key.path))
            .map(|draft| draft.stamp.clone())
            .collect(),
        ..Default::default()
    };
    if request.query.is_empty() && !request.missing_assets {
        return result;
    }
    if request.missing_assets && (!request.references || !request.folder.is_empty()) {
        result.error = "缺失检查必须使用config/scenes只读路径范围".into();
        return result;
    }
    if request.references && !request.missing_assets && (!references::valid_target(&request.query) || !request.folder.is_empty()) {
        result.error = "引用目标必须是assets内的物理文件路径".into();
        return result;
    }
    if Path::new(&request.folder)
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
    {
        result.error = "搜索目录必须位于assets内".into();
        return result;
    }
    let Ok(root) = request.root.canonicalize() else {
        result.error = "无法打开assets目录".into();
        return result;
    };
    let Ok(folder) = root.join(&request.folder).canonicalize() else {
        result.error = "搜索目录不存在，请刷新资产目录".into();
        return result;
    };
    if !folder.starts_with(&root) {
        result.error = "搜索目录超出assets范围".into();
        return result;
    }
    let query = request.query.clone();
    let case = request.case_sensitive;
    let whole = request.whole_word;
    let regex = if request.references {
        None
    } else {
        match AsyncComputeTaskPool::get()
            .spawn(async move { navigation::literal_regex(&query, case, whole) })
            .await
        {
            Ok(regex) => Some(regex),
            Err(_) => {
                result.error = "查找条件超过后台预算，请缩短查询".into();
                return result;
            }
        }
    };
    let mut total = 0;
    let mut metadata_checks = 0; // 全次检查共同计量，重复引用与深目录不能绕过最坏IO预算。
    for draft in &request.drafts {
        if request.cancel.load(Ordering::Relaxed) || result.limited {
            break;
        }
        // 后台也核验命名空间，防止旧请求把工程源码草稿伪装成资产路径结果。
        if source::is_project(&draft.stamp.key.path) { continue; }
        if in_folder(&draft.stamp.key.path, &request.folder)
            && if request.references { references::accepts(&draft.stamp.key.path) }
                else { accepts(&draft.stamp.key.path, &request.filters) }
        {
            add_file(
                &request,
                &mut result,
                &regex,
                &root,
                draft.stamp.key.path.clone(),
                draft.text.clone(),
                ContentIdentity::Draft(draft.stamp.clone()),
                draft.dirty,
                &mut total,
                &mut metadata_checks,
            )
            .await;
        }
    }
    let Ok(directory) = std::fs::read_dir(&folder) else {
        result.error = "无法读取搜索目录".into();
        return result;
    };
    // 流式DFS仅保存32层ReadDir，不收集整个资产树，也不跟随链接/reparse目录。
    let mut stack = vec![directory];
    let mut entries = 0;
    while !stack.is_empty() && !result.limited && !request.cancel.load(Ordering::Relaxed) {
        let Some(entry) = stack.last_mut().unwrap().next() else {
            stack.pop();
            continue;
        };
        entries += 1;
        if entries > MAX_ENTRIES {
            result.limited = true;
            break;
        }
        let Ok(entry) = entry else {
            result.skipped += 1;
            continue;
        };
        let path = entry.path();
        let Some(relative) = path.strip_prefix(&root).ok().and_then(|p| p.to_str()) else {
            result.skipped += 1;
            continue;
        };
        let relative = relative.replace('\\', "/");
        if relative.len() > MAX_PATH_BYTES {
            result.skipped += 1;
            continue;
        }
        // 固定只进入config/scenes，避免为查一个模型遍历纹理、源资产等无关目录。
        if request.references && !references::in_scope(&relative) {
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
            match std::fs::read_dir(path) {
                Ok(dir) => stack.push(dir),
                Err(_) => result.skipped += 1,
            }
            continue;
        }
        if !metadata.is_file()
            || !(if request.references { references::accepts(&relative) }
                else { accepts(&relative, &request.filters) })
            || request
                .drafts
                .iter()
                .any(|draft| draft.stamp.key.path == relative)
        {
            continue;
        }
        match io::load(&root, &relative) {
            Ok((disk, source)) => {
                add_file(
                    &request,
                    &mut result,
                    &regex,
                    &root,
                    relative,
                    source.text,
                    ContentIdentity::Disk(disk.digest),
                    false,
                    &mut total,
                    &mut metadata_checks,
                )
                .await
            }
            Err(_) => result.skipped += 1, // 单文件编码、大小、权限或并发错误不破坏其他完整结果。
        }
    }
    if request.references && !request.missing_assets && !result.limited && !request.cancel.load(Ordering::Relaxed) {
        references::scan_sources(&request, &mut result, &mut entries, &mut total).await;
    } // 同一IO槽依次扫描源码，不额外并发、不扩大普通内容搜索的assets边界。
    result.cancelled = request.cancel.load(Ordering::Relaxed);
    result.files.sort_by(|a, b| a.path.cmp(&b.path));
    for (file, group) in result.files.iter().enumerate() {
        result.rows.push(ResultRow::File(file));
        result
            .rows
            .extend((0..group.hits.len()).map(|hit| ResultRow::Hit(file, hit)));
    }
    result
}
