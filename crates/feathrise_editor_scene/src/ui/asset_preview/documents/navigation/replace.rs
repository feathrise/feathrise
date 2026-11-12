//! 后台字面替换与完整结果采用；原始字节、选区方向和独立一步历史成对更新。

use super::super::{
    components::DocumentSource,
    history::{EditContext, Selection},
    resources::DocumentState,
    types::ViewMode,
};
use super::{control::busy, index, resources::DocumentNavigation, types::*};
use crate::resources::{
    EditorTextInputGuard, SceneEditorNumericScrubMouseGuard, SceneEditorSettingsState, SceneEditorState,
};
use bevy::{
    input_focus::InputFocus,
    prelude::*,
    text::{EditableText, FontCx, LayoutCx},
};
use std::{ops::Range, sync::Arc};
use feathrise_editor_documents_model::DocumentContentOperations;

/// 只重写已匹配区间；跳过区间之间的原始字节，混合CRLF/LF不因全部替换而被统一。
pub(super) fn prepare_with(
    operations: DocumentContentOperations,
    source: &str,
    pages: &[Arc<str>],
    crlf: bool,
    bom: bool,
    request: &ReplacementRequest,
) -> Result<Option<ReplacedContent>, String> {
    let ranges: &[Range<usize>] = match &request.target {
        Some(range) => std::slice::from_ref(range),
        None => &request.index.matches,
    };
    (operations.replace_ranges)(
        source,
        pages,
        crlf,
        bom,
        ranges,
        &request.replacement,
        request.selection,
        request.index.key.scope.as_ref(),
        request.target.is_some(),
    )
}

#[cfg(test)]
pub(super) fn prepare(
    source: &str, pages: &[Arc<str>], crlf: bool, bom: bool, request: &ReplacementRequest,
) -> Result<Option<ReplacedContent>, String> {
    prepare_with(feathrise_editor_documents::content_operations(), source, pages, crlf, bom, request)
}

pub(super) fn apply(
    editor: Res<SceneEditorState>,
    settings: Option<Res<SceneEditorSettingsState>>,
    guard: Res<EditorTextInputGuard>,
    mouse: Res<SceneEditorNumericScrubMouseGuard>,
    focus: Res<InputFocus>,
    mut documents: ResMut<DocumentState>,
    mut state: ResMut<DocumentNavigation>,
    fields: Query<
        &EditableText,
        (
            With<super::components::NavigationField>,
            Without<DocumentSource>,
        ),
    >,
    mut sources: Query<(Entity, &DocumentSource, &mut EditableText)>,
    mut font: ResMut<FontCx>,
    mut layout: ResMut<LayoutCx>,
) {
    if !editor.open
        || editor.close_requested
        || editor.new_scene_dialog_open
        || settings.is_some_and(|settings| settings.open)
        || guard.block_shortcuts
        || mouse.blocked()
        || !documents.visible
        || documents.pending_load.is_some()
        || (documents.io.is_some() && documents.saving.is_none())
        || fields.iter().any(busy)
    {
        state.cancel_replace();
        return;
    }
    let Some(result) = state.ready.take() else {
        return;
    };
    state.replace = None;
    let request = result.request;
    let Some(doc) = documents.current().filter(|doc| {
        doc.mode == ViewMode::Edit
            && state.mode == NavigationMode::Replace
            && index::current(&request.index.key, doc, &state)
    }) else {
        return;
    };
    let Ok((entity, binding, mut text)) = sources.single_mut() else {
        return;
    };
    if entity != request.source || binding.key.as_ref() != Some(&doc.source_key()) {
        return;
    }
    if busy(&text) || Selection::read(&text) != request.selection || focus.get() != request.focus {
        state.hint = "输入、选区或焦点已变化，替换已取消；草稿保持".into();
        return;
    }
    match result.content {
        Ok(Some(content)) => {
            let selection = content.selection;
            let doc = documents.current_mut().unwrap();
            doc.replace_content(
                content.source,
                content.pages,
                EditContext {
                    before: request.selection,
                    after: selection,
                    ..default()
                },
            );
            doc.status = format!("已替换{}处；可一次撤销，保存后写入文件", content.count);
            if state.in_selection {
                state.scope = content.scope.map(|range| SelectionScope {
                    document: doc.source_key(),
                    revision: doc.revision,
                    range,
                }); // 仅本次受检替换可更新冻结范围；其他编辑或撤销使范围失效。
            }
            state.invalidate();
            state.hint = doc.status.clone();
            text.editor.set_text(doc.source_text());
            text.editor
                .driver(&mut font.context, &mut layout.0)
                .select_byte_range(selection.anchor, selection.focus);
            state.reveal = Some(RevealTarget {
                source: entity,
                key: index::key_for(doc, &state),
                selection: text.editor.raw_selection().text_range(),
            });
            if request.target.is_some() {
                state.pending = Some(NavigationRequest::Next(false));
            }
        }
        Ok(None) => state.hint = "匹配内容已相同；草稿保持".into(),
        Err(error) => state.hint = error,
    }
}
