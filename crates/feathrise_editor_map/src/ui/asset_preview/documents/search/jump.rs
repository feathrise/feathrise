//! 只读搜索结果通过既有文档标签加载，完整身份验证后选择并露出真实匹配。

use super::super::{
    components::{DocumentScrollState, DocumentSource},
    navigation,
    resources::DocumentState,
    types::{Document, ViewMode},
};
use super::{
    components::SearchLabel,
    control::{SearchGate, busy},
    resources::AssetSearchState,
    types::*,
};
use crate::resources::MapEditorState;
use bevy::{
    input_focus::{FocusCause, InputFocus},
    prelude::*,
    text::{EditableText, FontCx, LayoutCx, TextLayoutInfo},
};

pub(super) fn identity_matches(identity: &ContentIdentity, doc: &Document) -> bool {
    match identity {
        ContentIdentity::Draft(stamp) => {
            doc.source_key() == stamp.key && doc.revision == stamp.revision
        }
        ContentIdentity::Disk(digest) => &doc.disk.digest == digest && doc.revision == 0,
        ContentIdentity::ProjectSource(_) => false,
    }
}

pub(super) fn prepare(
    editor: Res<MapEditorState>,
    gate: SearchGate,
    mut documents: ResMut<DocumentState>,
    mut state: ResMut<AssetSearchState>,
) {
    let Some(jump) = &state.jump else {
        return;
    };
    if gate.blocked(&editor)
        || documents
            .expanded
            .as_ref()
            .is_none_or(|tab| tab.path != jump.path)
    {
        state.jump = None;
        return;
    }
    if documents.is_loading(&jump.path) {
        return; // 保留定位意图，载入成功后再校验新正文；不得把待丢弃的旧正文切回编辑。
    }
    let Some(doc) = documents.current_mut().filter(|doc| doc.path == jump.path) else {
        if !documents.error.is_empty() {
            state.hint = documents.error.clone();
            state.jump = None;
        }
        return;
    };
    if !identity_matches(&jump.identity, doc) || doc.source.get(jump.range.clone()).is_none() {
        state.hint = "文件在搜索后已变化，请重新搜索；当前草稿已保留".into();
        state.jump = None;
        return;
    }
    if doc.mode != ViewMode::Edit {
        doc.mode = ViewMode::Edit;
    } // 预览和Markdown结果也进入同一个源码输入，不创建第二套编辑器。
}

pub(super) fn select(
    editor: Res<MapEditorState>,
    gate: SearchGate,
    documents: Res<DocumentState>,
    mut state: ResMut<AssetSearchState>,
    inputs: Query<&EditableText, Without<DocumentSource>>,
    mut sources: Query<(Entity, &DocumentSource, &mut EditableText)>,
    mut labels: Query<(&SearchLabel, &mut Text)>,
    mut font: ResMut<FontCx>,
    mut layout: ResMut<LayoutCx>,
    mut focus: ResMut<InputFocus>,
) {
    if state.jump.is_none() {
        return;
    }
    if gate.blocked(&editor) || inputs.iter().any(busy) || focus.get().is_some() {
        state.jump = None;
        return;
    }
    let jump = state.jump.as_ref().unwrap();
    let Some(doc) = documents
        .current()
        .filter(|doc| documents.visible && doc.mode == ViewMode::Edit && doc.path == jump.path)
    else {
        return;
    };
    let Ok((entity, binding, mut text)) = sources.single_mut() else {
        return;
    };
    if binding.key.as_ref() != Some(&doc.source_key()) {
        return;
    }
    if text.is_composing() || text.pending_paste.is_some() || !identity_matches(&jump.identity, doc)
    {
        state.jump = None;
        return;
    }
    // 新文件绑定会排队TextStart；先让官方队列在下一帧完成。真实改字会使内容身份失效，
    // 手工接管焦点则在上面的检查取消，不能为等待初始化而覆盖作者后续输入。
    if !text.pending_edits.is_empty() {
        return;
    }
    let range = jump.range.clone();
    text.editor
        .driver(&mut font.context, &mut layout.0)
        .select_byte_range(range.start, range.end);
    focus.set(entity, FocusCause::Navigated);
    state.reveal = Some(JumpReveal {
        source: entity,
        document: doc.source_key(),
        revision: doc.revision,
        range,
    });
    // 中央文档激活会重排同一F12根；未变的结果文案也需刷新原生文字测量，
    // 避免沿用激活前的排版缓存而漏绘。仅成功定位时更新固定16行，不在稳定帧刷新。
    for (label, mut text) in &mut labels {
        if matches!(label, SearchLabel::Row(_)) {
            text.set_changed();
        }
    }
    state.jump = None;
    state.hint = "已在中央定位匹配 · 搜索结果保持只读".into();
}

pub(super) fn reveal(
    documents: Res<DocumentState>,
    mut state: ResMut<AssetSearchState>,
    mut sources: Query<(
        Entity,
        &DocumentSource,
        &mut EditableText,
        &ComputedNode,
        &TextLayoutInfo,
        &mut DocumentScrollState,
    )>,
) {
    if state.reveal.is_none() {
        return;
    }
    let target = state.reveal.take().unwrap();
    let Some(doc) = documents.current().filter(|doc| {
        documents.visible && doc.source_key() == target.document && doc.revision == target.revision
    }) else {
        return;
    };
    let Ok((entity, binding, mut text, node, layout, mut preservation)) =
        sources.single_mut()
    else {
        return;
    };
    if entity == target.source
        && binding.key.as_ref() == Some(&doc.source_key())
        && !busy(&text)
        && text.editor.raw_selection().text_range() == target.range
    {
        navigation::reveal_selection(node, layout, &mut text.viewport, &mut preservation);
    }
}
