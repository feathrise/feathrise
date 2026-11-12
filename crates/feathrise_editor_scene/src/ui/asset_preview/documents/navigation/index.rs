//! 共享Arc草稿后台生成逻辑行索引和完整字面匹配；迟到结果必须校验整份身份。

use super::super::{
    resources::DocumentState,
    types::{Document, ViewMode},
};
use super::{replace, resources::DocumentNavigation, types::*};
use feathrise_editor_documents_model::DocumentContentOperations;
use bevy::{
    prelude::*,
    tasks::{AsyncComputeTaskPool, futures::check_ready},
};
use std::sync::Arc;

pub(super) fn current(key: &IndexKey, doc: &Document, state: &DocumentNavigation) -> bool {
    key.document.path == doc.path
        && key.document.generation == doc.generation
        && key.revision == doc.revision
        && key.ticket == state.ticket
        && key.query == state.query
        && key.options == state.options
        && key.scope == state.scope_for(doc)
}

pub(super) fn key_for(doc: &Document, state: &DocumentNavigation) -> IndexKey {
    IndexKey {
        document: doc.source_key(),
        revision: doc.revision,
        query: state.query.clone(),
        ticket: state.ticket,
        options: state.options,
        scope: state.scope_for(doc),
    }
}

pub(in super::super) use feathrise_editor_documents_model::navigation::query::literal_regex;

#[cfg(test)]
pub(super) fn build(key: IndexKey, source: &str) -> TextIndex {
    (feathrise_editor_documents::content_operations().build_text_index)(key, source)
}

pub(super) fn drive(
    documents: Res<DocumentState>,
    operations: Res<DocumentContentOperations>,
    mut state: ResMut<DocumentNavigation>,
) {
    let operations = *operations;
    let doc = documents
        .current()
        .filter(|doc| documents.visible && doc.mode == ViewMode::Edit);
    if let Some(result) = state.task.as_mut().and_then(check_ready) {
        state.task = None;
        match result {
            NavigationWork::Index(result)
                if doc.is_some_and(|doc| current(&result.key, doc, &state)) =>
            {
                state.index = Some(Arc::new(result));
                state.selected = None;
            }
            NavigationWork::Replace(result)
                if doc.is_some_and(|doc| current(&result.request.index.key, doc, &state))
                    && state
                        .replace
                        .as_ref()
                        .is_some_and(|request| request.ticket == result.request.ticket) =>
            {
                state.ready = Some(result);
            }
            _ => (),
        } // 未完成旧任务始终占唯一槽；关闭、换代后仅丢弃结果，不登记任何资产。
    }
    let Some(doc) = doc else {
        state.cancel_replace();
        return;
    };
    if state
        .replace
        .as_ref()
        .is_some_and(|request| !current(&request.index.key, doc, &state))
    {
        state.cancel_replace();
    }
    if state.task.is_some() || state.ready.is_some() {
        return;
    }
    if let Some(request) = state.replace.clone() {
        let source = Arc::clone(&doc.source);
        let pages = doc.pages.clone();
        let crlf = doc.disk.crlf;
        let bom = doc.disk.bom;
        state.task = Some(AsyncComputeTaskPool::get().spawn(async move {
            let content = replace::prepare_with(operations, &source, &pages, crlf, bom, &request);
            NavigationWork::Replace(ReplacementResult { request, content })
        }));
        return;
    }
    if state
        .index
        .as_ref()
        .is_some_and(|index| current(&index.key, doc, &state))
    {
        return;
    }
    state.index = None;
    let key = key_for(doc, &state);
    let source = Arc::clone(&doc.source); // 不在任务启动帧复制全文。
    state.task = Some(
        AsyncComputeTaskPool::get()
            .spawn(async move { NavigationWork::Index((operations.build_text_index)(key, &source)) }),
    );
}

pub(super) fn next_match(
    matches: &[std::ops::Range<usize>],
    selected: Option<usize>,
    caret: usize,
    back: bool,
) -> Option<usize> {
    if matches.is_empty() {
        return None;
    }
    Some(
        if let Some(selected) = selected.filter(|selected| *selected < matches.len()) {
            if back {
                (selected + matches.len() - 1) % matches.len()
            } else {
                (selected + 1) % matches.len()
            }
        } else if back {
            matches
                .partition_point(|range| range.start < caret)
                .checked_sub(1)
                .unwrap_or(matches.len() - 1)
        } else {
            let next = matches.partition_point(|range| range.start < caret);
            if next == matches.len() { 0 } else { next }
        },
    )
}
