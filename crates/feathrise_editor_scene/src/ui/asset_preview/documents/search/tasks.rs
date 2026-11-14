//! 搜索任务非阻塞采用及输入身份核验；改变条件、草稿或目录不会采用迟到结果。

use super::super::{resources::DocumentState, source};
use super::{resources::AssetSearchState, types::*, worker};
use crate::resources::{EditorAssetCatalog, SceneEditorState};
use bevy::{
    prelude::*,
    tasks::{IoTaskPool, futures::check_ready},
};
use std::sync::Arc;

fn drafts_current(stamps: &[DraftStamp], documents: &DocumentState) -> bool {
    stamps.iter().all(|stamp| {
        documents.documents.iter().any(|doc| {
            doc.path == stamp.key.path
                && doc.generation == stamp.key.generation
                && doc.revision == stamp.revision
        })
    }) && documents.documents.iter().all(|doc| {
        source::is_project(&doc.path) || !doc.dirty()
            || stamps.iter().any(|stamp| {
                stamp.key.path == doc.path
                    && stamp.key.generation == doc.generation
                    && stamp.revision == doc.revision
            })
    })
}

fn request_drafts_current(request: &SearchRequest, documents: &DocumentState) -> bool {
    request.drafts.iter().all(|draft| {
        documents.documents.iter().any(|doc| {
            doc.path == draft.stamp.key.path
                && doc.generation == draft.stamp.key.generation
                && doc.revision == draft.stamp.revision
        })
    }) && documents.documents.iter().all(|doc| {
        // 未纳入资产快照的工程源码变更不使本次资产查询失效。
        source::is_project(&doc.path) || !doc.dirty()
            || request.drafts.iter().any(|draft| {
                draft.stamp.key.path == doc.path
                    && draft.stamp.key.generation == doc.generation
                    && draft.stamp.revision == doc.revision
            })
    })
}

pub(super) fn drive(
    editor: Res<SceneEditorState>,
    catalog: Res<EditorAssetCatalog>,
    documents: Res<DocumentState>,
    mut state: ResMut<AssetSearchState>,
) {
    let stale = state
        .pending
        .as_ref()
        .or(state.running.as_ref())
        .filter(|request| request.ticket == state.ticket)
        .is_some_and(|request| {
            request.root != documents.root
                || (!request.references && state.current_folder && request.folder.as_str() != state.host_folder.as_deref().unwrap_or(&catalog.current_folder))
                || !request_drafts_current(request, &documents)
        })
        || state.current().is_some_and(|result| {
            result.root != documents.root
                || (!result.references && state.current_folder && result.folder.as_str() != state.host_folder.as_deref().unwrap_or(&catalog.current_folder))
                || !drafts_current(&result.drafts, &documents)
        });
    if stale {
        state.invalidate("文档或目录已变化，请重新搜索或查找引用");
    }
    if (!editor.open || editor.close_requested)
        && (state.reference_requested || state.pending.is_some()
            || state
                .running
                .as_ref()
                .is_some_and(|request| request.ticket == state.ticket))
    {
        let hint = if state.missing_assets { "缺失检查已取消，点击重新检查" }
            else if state.reference_target.is_some() { "引用查询已取消，点击重新查找" }
            else { "搜索已取消，按Enter重新搜索" };
        state.invalidate(hint);
    }
    if state.task.is_some()
        && let Some(result) = state.task.as_mut().and_then(check_ready)
    {
        state.task = None;
        state.running = None;
        if result.ticket == state.ticket && !result.cancelled {
            let hits = result.hits;
            state.hint = if !result.error.is_empty() {
                result.error.clone()
            } else if result.missing_assets {
                format!("{}处路径问题 · {}个文件 · 已检查{}条路径、{}个JSON{}{} · 点击定位",
                    hits, result.files.len(), result.checked_paths, result.scanned,
                    if result.skipped > 0 { format!(" · 跳过{}项（JSON/编码/读取失败或超限）", result.skipped) } else { String::new() },
                    if result.limited || result.skipped > 0 { " · 检查未完成，已列出项仍可定位" } else { "" })
            } else {
                format!(
                    "{}处{} · {}个文件 · 已搜索{}个文件{}{} · 滚轮浏览，点击定位",
                    hits,
                    if result.references { "直接引用" } else { "匹配" },
                    result.files.len(),
                    result.scanned,
                    if result.skipped > 0 {
                        format!(" · 跳过{}项（超限/编码/链接/{}读取失败）", result.skipped,
                            if result.references { "无效JSON/" } else { "" })
                    } else {
                        String::new()
                    },
                    if result.limited && result.references {
                        " · 引用未全部列出，已列出项仍可定位"
                    } else if result.limited {
                        " · 结果未全部列出，已列出项仍可定位或替换"
                    } else {
                        ""
                    }
                )
            };
            state.reset_scroll = true;
        }
        // 即便迟到也先O(1)保留；下一任务在后台释放，不能在采用帧析构可变长度结果。
        state.result = Some(Arc::new(result));
    }
    if state.task.is_none()
        && state.pending.is_some()
        && let Some(request) = state.pending.take()
    {
        let retired = state.result.take();
        state.running = Some(request.clone());
        state.task = Some(IoTaskPool::get().spawn(worker::run(request, retired)));
    }
}
