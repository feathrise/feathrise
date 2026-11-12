//! 完整分词仅在AsyncCompute单槽执行；隐藏、换文件或编辑后的迟到结果只丢弃。

use super::super::resources::DocumentState;
use super::{resources::SyntaxIndex, types::*};
use feathrise_editor_documents_model::DocumentContentOperations;
use bevy::{
    prelude::*,
    tasks::{AsyncComputeTaskPool, futures::check_ready},
};
use std::sync::Arc;

pub(super) fn drive(
    documents: Res<DocumentState>,
    operations: Res<DocumentContentOperations>,
    mut state: ResMut<SyntaxIndex>,
) {
    let doc = documents
        .current()
        .filter(|doc| documents.visible && Language::from_path(&doc.path).is_some());
    if let Some(result) = state.task.as_mut().and_then(check_ready) {
        state.task = None;
        if doc.is_some_and(|doc| result.key.current(doc)) {
            state.result = Some(result);
        }
    }
    if state
        .result
        .as_ref()
        .is_some_and(|result| doc.is_none_or(|doc| !result.key.current(doc)))
    {
        state.result = None;
    }
    let Some(doc) = doc else {
        return;
    };
    if state.task.is_some() || state.result.is_some() {
        return;
    }
    let key = SyntaxKey::new(doc);
    let source = Arc::clone(&doc.source); // 启动帧只共享不可变草稿；不复制全文，也不取消并重启占槽任务。
    let tokenize = operations.tokenize;
    state.task = Some(AsyncComputeTaskPool::get().spawn(async move {
        let tokens = tokenize(key.language.unwrap(), &source);
        SyntaxResult { key, tokens }
    }));
}
