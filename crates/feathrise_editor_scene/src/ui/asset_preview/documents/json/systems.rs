//! 校验缓存、显式保存意图与任务背压；旧任务真正完成前不释放后台槽。

use super::super::{resources::DocumentState, types::ViewMode};
use super::types::*;
use bevy::{
    prelude::*,
    tasks::{AsyncComputeTaskPool, futures::check_ready},
};

/// 按钮与Ctrl+S共用入口；未知校验排队，错误不写盘，普通文本沿用原保存流程。
pub(in super::super) fn before_save(state: &mut DocumentState, path: &str) -> bool {
    let doc = state.documents.iter().find(|doc| doc.path == path).unwrap();
    if !supported(path) {
        return true;
    }
    let key = JsonKey::new(doc);
    let message = if state.json.format.is_some() || state.json.ready.is_some() {
        Some("请先等待JSON格式化完成，再保存".to_owned())
    } else if state.json.save.is_some() {
        Some("JSON保存校验进行中，请稍候".to_owned())
    } else if let Some(checked) = state.json.checked(doc) {
        if let Some(error) = &checked.error {
            Some(format!(
                "{}；保存已停止，草稿保留，请修正后再保存",
                error.summary()
            ))
        } else {
            return true;
        }
    } else {
        state.json.save = Some(key);
        Some("正在后台检查JSON语法，通过后保存完整文档…".to_owned())
    };
    if let Some(message) = message {
        state
            .documents
            .iter_mut()
            .find(|doc| doc.path == path)
            .unwrap()
            .status = message;
    }
    false
}

pub(in super::super) fn drive(
    mut state: ResMut<DocumentState>,
    content: Res<feathrise_editor_documents_model::DocumentContentOperations>,
) {
    if state.json.task.is_some()
        && let Some(result) = state.json.task.as_mut().and_then(check_ready)
    {
        state.json.task = None;
        if state
            .documents
            .iter()
            .any(|doc| result.validation.key.current(doc))
        {
            state
                .json
                .cache
                .retain(|entry| entry.key.document != result.validation.key.document);
            state.json.cache.push(Validation {
                key: result.validation.key.clone(),
                error: result.validation.error.clone(),
            });
            if result.request.as_ref().is_some_and(|request| {
                state.json.format.as_ref().is_some_and(|wanted| {
                    wanted.ticket == request.ticket && wanted.key == request.key
                })
            }) {
                state.json.ready = Some(result);
            }
        }
    }
    // 至多八份会话的身份比较；稳定帧不扫描源文或保存字节。
    if state
        .json
        .cache
        .iter()
        .any(|entry| !state.documents.iter().any(|doc| entry.key.current(doc)))
    {
        let state_ref = &mut *state;
        state_ref
            .json
            .cache
            .retain(|entry| state_ref.documents.iter().any(|doc| entry.key.current(doc)));
    }
    if state.json.format.as_ref().is_some_and(|request| {
        !state.visible
            || state
                .current()
                .is_none_or(|doc| !request.key.current(doc) || doc.mode != ViewMode::Edit)
    }) {
        state.json.format = None;
        state.json.ready = None; // 换文件、隐藏或新输入取消采用，但在途task仍然占槽。
    }
    if let Some(key) = state.json.save.clone() {
        let doc = state.documents.iter().find(|doc| key.current(doc));
        if doc.is_none() {
            state.json.save = None;
            if let Some(doc) = state
                .documents
                .iter_mut()
                .find(|doc| doc.source_key() == key.document)
            {
                doc.status = "校验期间正文已变化，保存已取消；新草稿保留，请再次保存".into();
            }
        } else if let Some(checked) = state.json.checked(doc.unwrap()) {
            if let Some(error) = &checked.error {
                let message = format!("{}；保存已停止，草稿保留，请修正后再保存", error.summary());
                state.json.save = None;
                state
                    .documents
                    .iter_mut()
                    .find(|doc| key.current(doc))
                    .unwrap()
                    .status = message;
            } else if state.io.is_none() && state.pending_load.is_none() {
                state.json.save = None;
                state.start_save(&key.document.path); // 仍是用户点击时的完整修订，外部磁盘身份由原事务复检。
            }
        }
    }
    if state.json.task.is_some() || state.json.ready.is_some() {
        return;
    }
    let key = state
        .json
        .save
        .clone()
        .or_else(|| {
            state
                .json
                .format
                .as_ref()
                .map(|request| request.key.clone())
        })
        .or_else(|| {
            state
                .current()
                .filter(|doc| {
                    state.visible && supported(&doc.path) && state.json.checked(doc).is_none()
                })
                .map(JsonKey::new)
        });
    let Some(key) = key else {
        return;
    };
    let Some(doc) = state.documents.iter().find(|doc| key.current(doc)) else {
        return;
    };
    let source = doc.source.clone();
    let crlf = doc.disk.crlf;
    let bom = doc.disk.bom;
    let request = state
        .json
        .format
        .as_ref()
        .filter(|request| request.key == key)
        .cloned();
    let process = content.process_json;
    state.json.task = Some(AsyncComputeTaskPool::get().spawn(async move {
        let processed = process(
            &source, crlf, bom, request.as_ref().map(|request| request.selection),
        );
        WorkResult {
            validation: Validation { key, error: processed.error },
            request,
            formatted: processed.formatted,
        }
    }));
}
