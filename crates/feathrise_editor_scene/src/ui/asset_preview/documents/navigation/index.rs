//! 共享Arc草稿后台生成逻辑行索引和完整字面匹配；迟到结果必须校验整份身份。

use super::super::{
    resources::DocumentState,
    types::{Document, ViewMode},
};
use super::{constants::REGEX_BYTES, replace, resources::DocumentNavigation, types::*};
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

/// 当前文件与跨文件搜索共享转义、Unicode大小写和外侧词界，避免两种入口结果不同。
pub(in super::super) fn literal_regex(
    query: &str,
    case_sensitive: bool,
    whole_word: bool,
) -> Result<regex::Regex, regex::Error> {
    let escaped = regex::escape(query);
    let pattern = if whole_word {
        format!(r"\b{{start-half}}{escaped}\b{{end-half}}")
    } else {
        escaped
    };
    regex::RegexBuilder::new(&pattern)
        .case_insensitive(!case_sensitive)
        .size_limit(REGEX_BYTES)
        .dfa_size_limit(REGEX_BYTES)
        .build()
}

pub(super) fn build(key: IndexKey, source: &str) -> TextIndex {
    // 输入已统一为LF。末尾换行额外产生真实空行；字节索引可直接交给原生Parley导航。
    let mut lines = vec![0];
    lines.extend(
        source
            .bytes()
            .enumerate()
            .filter_map(|(i, ch)| (ch == b'\n').then_some(i + 1)),
    );
    let (matches, error) = if key.query.is_empty() {
        (Vec::new(), None)
    } else {
        // 用户始终输入字面文字；Unicode大小写和组合字符词界交给已锁定的成熟引擎。
        // 半词界只检查两侧，不把含引号、点号或斜线的查询强制限定为一个标识符。
        match literal_regex(
            &key.query,
            key.options.case_sensitive,
            key.options.whole_word,
        ) {
            Ok(regex) => {
                let scope = key.scope.clone().unwrap_or(0..source.len());
                let mut matches = Vec::new();
                let mut cursor = scope.start;
                // 从冻结范围起点开始，不能先让范围外的重叠匹配吃掉范围内候选；
                // find_at保留整篇上下文，选区边缘也不能伪造一个新的词界。
                while cursor < scope.end && cursor < source.len() {
                    let Some(found) = regex.find_at(source, cursor) else {
                        break;
                    };
                    if found.end() > scope.end {
                        break;
                    }
                    matches.push(found.range());
                    cursor = found.end(); // 非空字面查询始终前进，替换候选互不重叠。
                }
                (matches, None)
            }
            Err(_) => (
                Vec::new(),
                Some("查找条件超过后台预算，请缩短查询后重试".into()),
            ),
        }
    };
    TextIndex {
        key,
        lines,
        matches,
        error,
    }
}

pub(super) fn drive(documents: Res<DocumentState>, mut state: ResMut<DocumentNavigation>) {
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
            let content = replace::prepare(&source, &pages, crlf, bom, &request);
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
            .spawn(async move { NavigationWork::Index(build(key, &source)) }),
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
