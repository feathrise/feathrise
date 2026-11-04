//! 在IO池核验全部磁盘身份，串行计算完整保真替换与片段；不写文件、不产生局部结果。

use super::super::super::{
    constants::MAX_DOCUMENTS,
    io, navigation,
    types::{DiskIdentity, SourceContents},
};
use super::super::{constants::MAX_HITS, types::ContentIdentity, worker::match_text};
use super::{constants::REPLACEMENT_CHARS, types::*};
use bevy::tasks::AsyncComputeTaskPool;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

fn cancelled(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Relaxed) {
        Err("替换已取消；所有草稿保持".into())
    } else {
        Ok(())
    }
}

fn verify_disk(expected: &DiskIdentity, actual: &DiskIdentity) -> Result<(), String> {
    if expected.path != actual.path || expected.digest != actual.digest {
        Err("磁盘文件在搜索或预览后已变化，请重新载入并搜索；草稿保持".into())
    } else {
        Ok(())
    }
}

/// 并列片段保留同一上下文，替换文字中的美元、反斜线及空值始终按字面显示。
fn after_snippet(source: &str, range: std::ops::Range<usize>, replacement: &str) -> String {
    let start = source[..range.start]
        .char_indices()
        .rev()
        .take_while(|(_, ch)| *ch != '\n')
        .take(24)
        .last()
        .map_or(range.start, |(i, _)| i);
    let left: String = source[start..range.start]
        .chars()
        .map(|ch| if ch == '\t' { ' ' } else { ch })
        .collect();
    let mut value: String = replacement.chars().take(128).collect();
    if replacement.chars().nth(128).is_some() {
        value.push('…');
    }
    let right: String = source[range.end..]
        .chars()
        .take_while(|ch| *ch != '\n')
        .take(48)
        .map(|ch| if ch == '\t' { ' ' } else { ch })
        .collect();
    format!("{left}‹{value}›{right}")
}

fn prepare_file(
    index: usize,
    search: &super::super::types::SearchResult,
    loaded: (DiskIdentity, SourceContents),
    draft: Option<DraftInput>,
    regex: &regex::Regex,
    replacement: &str,
    cancel: &AtomicBool,
) -> Result<PreviewFile, String> {
    cancelled(cancel)?;
    let file = &search.files[index];
    let (disk, loaded) = loaded;
    let (stamp, source, pages, selection) = if let Some(draft) = draft {
        verify_disk(&draft.disk, &disk)?;
        let valid = match &file.identity {
            ContentIdentity::Draft(stamp) => *stamp == draft.stamp,
            ContentIdentity::Disk(digest) => {
                *digest == draft.disk.digest && draft.stamp.revision == 0
            }
            ContentIdentity::ProjectSource(_) => false, // 源码路径结果不进入内置草稿替换。
        };
        if !valid {
            return Err("文档草稿在搜索后已变化，请重新搜索".into());
        }
        (
            Some(draft.stamp),
            draft.source,
            draft.pages,
            draft.selection,
        )
    } else {
        if !matches!(&file.identity, ContentIdentity::Disk(digest) if *digest == disk.digest) {
            return Err("搜索内容已失效，请重新搜索；所有草稿保持".into());
        }
        (None, loaded.text, loaded.pages, Default::default())
    };
    // 达到搜索上限只收紧为“已列出项”，不禁用操作，也不偷偷补入未预览的匹配。
    // 核验同一内容的真实匹配前缀；完整搜索仍核验全部集合，所有磁盘/草稿身份门保留。
    let maximum = if search.limited {
        file.hits.len()
    } else {
        MAX_HITS + 1
    };
    let hits = match_text(regex, &source, maximum, cancel);
    if hits.len() != file.hits.len() || hits.iter().zip(&file.hits).any(|(a, b)| a.range != b.range)
    {
        return Err("已列出的匹配集合已变化，请重新搜索；所有草稿保持".into());
    }
    let ranges: Vec<_> = hits.iter().map(|hit| hit.range.clone()).collect();
    let content = navigation::prepare_ranges(
        &source,
        &pages,
        disk.crlf,
        disk.bom,
        &ranges,
        replacement,
        selection,
        None,
        false,
    )?;
    let after = hits
        .iter()
        .map(|hit| after_snippet(&source, hit.range.clone(), replacement))
        .collect();
    cancelled(cancel)?;
    Ok(PreviewFile {
        index,
        draft: stamp,
        disk,
        source,
        pages,
        selection,
        content,
        after,
    })
}

async fn preview(request: &Request) -> Result<Arc<ReplacePlan>, String> {
    let Work::Preview {
        search,
        query,
        replacement,
        case_sensitive,
        whole_word,
        checked,
        drafts,
    } = &request.work
    else {
        unreachable!()
    };
    if search.references || search.missing_assets {
        return Err("资产路径查询只读，请使用内容搜索后再生成替换预览".into());
    }
    if !search.error.is_empty() || search.cancelled {
        return Err("搜索失败或已取消，请重新搜索后替换；所有草稿保持".into());
    }
    if checked.is_empty()
        || checked.len() > MAX_DOCUMENTS
        || replacement.chars().count() > REPLACEMENT_CHARS
    {
        return Err("请勾选1至8个文件，替换文字最多512个字符".into());
    }
    let (query, case_sensitive, whole_word) = (query.clone(), *case_sensitive, *whole_word);
    let regex = AsyncComputeTaskPool::get()
        .spawn(async move {
            navigation::literal_regex(&query, case_sensitive, whole_word)
                .map_err(|_| "查询超过计算预算".to_owned())
        })
        .await?;
    let mut files = Vec::new();
    for &index in checked {
        cancelled(&request.cancel)?;
        let file = search.files.get(index).ok_or("搜索文件身份已失效")?;
        if files.iter().any(|file: &PreviewFile| file.index == index) {
            return Err("不能重复选择同一文件".into());
        }
        let loaded = io::load(&search.root, &file.path)
            .map_err(|error| format!("{}：{error}", file.path))?;
        let draft = drafts
            .iter()
            .find(|draft| draft.stamp.key.path == file.path)
            .cloned();
        let (search, regex, replacement, cancel) = (
            search.clone(),
            regex.clone(),
            replacement.clone(),
            request.cancel.clone(),
        );
        files.push(
            AsyncComputeTaskPool::get()
                .spawn(async move {
                    prepare_file(index, &search, loaded, draft, &regex, &replacement, &cancel)
                })
                .await?,
        );
    }
    cancelled(&request.cancel)?;
    Ok(Arc::new(ReplacePlan {
        ticket: request.ticket,
        search_ticket: search.ticket,
        root: search.root.clone(),
        search: search.clone(),
        replacement: replacement.clone(),
        files,
    }))
}

fn copy_content(content: &navigation::ReplacedContent) -> navigation::ReplacedContent {
    navigation::ReplacedContent {
        source: content.source.clone(),
        pages: content.pages.clone(),
        selection: content.selection,
        scope: None,
        count: content.count,
    }
}

async fn apply(
    request: &Request,
    plan: &Arc<ReplacePlan>,
    target: Target,
) -> Result<Vec<AppliedFile>, String> {
    let mut content = Vec::new();
    let mut found = false;
    for file in &plan.files {
        let included = match target {
            Target::Checked => true,
            Target::File(index) | Target::Hit(index, _) => index == file.index,
        };
        if !included {
            continue;
        }
        found = true;
        cancelled(&request.cancel)?;
        let path = &plan.search.files[file.index].path;
        let (disk, _) = io::load(&plan.root, path).map_err(|error| format!("{path}：{error}"))?;
        verify_disk(&file.disk, &disk)?;
        let replacement = if let Target::Hit(_, hit) = target {
            let range = plan.search.files[file.index]
                .hits
                .get(hit)
                .ok_or("匹配身份已失效")?
                .range
                .clone();
            let (source, pages, replacement, selection) = (
                file.source.clone(),
                file.pages.clone(),
                plan.replacement.clone(),
                file.selection,
            );
            AsyncComputeTaskPool::get()
                .spawn(async move {
                    navigation::prepare_ranges(
                        &source,
                        &pages,
                        disk.crlf,
                        disk.bom,
                        &[range],
                        &replacement,
                        selection,
                        None,
                        true,
                    )
                })
                .await?
        } else {
            file.content.as_ref().map(copy_content)
        };
        if let Some(replacement) = replacement {
            content.push(AppliedFile {
                index: file.index,
                content: replacement,
            });
        }
    }
    cancelled(&request.cancel)?;
    if !found {
        return Err("当前文件未纳入这次预览，请勾选并更新预览".into());
    }
    Ok(content)
}

pub(super) async fn run(
    request: Request,
    retired: Option<Outcome>,
    old_plan: Option<Arc<ReplacePlan>>,
    old_request: Option<Request>,
) -> Reply {
    // 所有可变规模析构在后台发生；取消不丢弃计算中的句柄，也不提前归还槽。
    drop(retired);
    drop(old_plan);
    drop(old_request);
    let outcome = match &request.work {
        Work::Preview { .. } => Outcome::Preview(preview(&request).await),
        Work::Apply { plan, target } => Outcome::Apply {
            plan: plan.clone(),
            content: apply(&request, plan, *target).await,
        },
    };
    Reply {
        ticket: request.ticket,
        outcome,
    }
}
