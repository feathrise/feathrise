//! JSON显式工具操作的输入仲裁与完整结果采用；正文仍只用原生单一输入。

use super::super::{
    components::{DocumentScrollState, DocumentSource},
    history::{EditContext, Selection},
    resources::DocumentState,
    types::ViewMode,
};
use super::{components::JsonAction, types::*};
use crate::resources::{
    EditorTextInputGuard, MapEditorNumericScrubMouseGuard, MapEditorSettingsState, MapEditorState,
};
use bevy::{
    input_focus::{FocusCause, InputFocus},
    prelude::*,
    text::{EditableText, FontCx, LayoutCx, TextLayoutInfo},
};

fn blocked(
    editor: &MapEditorState,
    settings: Option<&MapEditorSettingsState>,
    guard: &EditorTextInputGuard,
    mouse: &MapEditorNumericScrubMouseGuard,
) -> bool {
    !editor.open
        || editor.close_requested
        || editor.new_map_dialog_open
        || settings.is_some_and(|settings| settings.open)
        || guard.block_shortcuts
        || mouse.blocked()
}

fn busy(text: &EditableText) -> bool {
    text.is_composing() || text.pending_paste.is_some() || !text.pending_edits.is_empty()
}

pub(super) fn input(
    editor: Res<MapEditorState>,
    settings: Option<Res<MapEditorSettingsState>>,
    guard: Res<EditorTextInputGuard>,
    mouse: Res<MapEditorNumericScrubMouseGuard>,
    mut documents: ResMut<DocumentState>,
    mut focus: ResMut<InputFocus>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    sources: Query<(Entity, &DocumentSource, &EditableText)>,
    buttons: Query<(&JsonAction, &feathrise_editor_ui::EditorButtonClick), Changed<feathrise_editor_ui::EditorButtonClick>>,
) {
    if blocked(&editor, settings.as_deref(), &guard, &mouse)
        || !documents.visible
        || documents.pending_load.is_some()
        || (documents.io.is_some() && documents.saving.is_none())
    {
        return;
    }
    let Some(doc) = documents.current().filter(|doc| supported(&doc.path)) else {
        return;
    };
    let Ok((entity, binding, text)) = sources.single() else {
        return;
    };
    if busy(text) {
        return;
    }
    let mut action = buttons
        .iter()
        .find(|(_, pressed)| pressed.is_clicked())
        .map(|(action, _)| *action);
    let format_key = focus.get() == Some(entity)
        && (keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight))
        && (keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight))
        && keys.just_pressed(KeyCode::KeyF);
    if format_key {
        action = Some(JsonAction::Format);
        keys.clear_just_pressed(KeyCode::KeyF); // 只消费正文的Windows标准格式化快捷键，不影响查找框。
    }
    match action {
        Some(JsonAction::Format)
            if doc.mode == ViewMode::Edit && binding.key.as_ref() == Some(&doc.source_key()) =>
        {
            if documents.json.format.is_some() || documents.json.ready.is_some() {
                return;
            }
            let key = JsonKey::new(doc);
            let selection = Selection::read(text);
            documents.json.ticket = documents.json.ticket.wrapping_add(1);
            documents.json.format = Some(FormatRequest {
                key,
                ticket: documents.json.ticket,
                source: entity,
                selection,
            });
            let doc = documents.current_mut().unwrap();
            doc.confirm_reload = false;
            doc.history.break_group();
            doc.status = "正在后台格式化JSON…".into();
            focus.set(entity, FocusCause::Navigated);
        }
        Some(JsonAction::Locate)
            if documents
                .json
                .checked(doc)
                .is_some_and(|checked| checked.error.is_some()) =>
        {
            let key = JsonKey::new(doc);
            documents.json.locate = Some(key);
            documents.current_mut().unwrap().mode = ViewMode::Edit; // 预览中的诊断也能直接进入源码定位。
        }
        _ => (),
    }
}

pub(super) fn apply(
    editor: Res<MapEditorState>,
    settings: Option<Res<MapEditorSettingsState>>,
    guard: Res<EditorTextInputGuard>,
    mouse: Res<MapEditorNumericScrubMouseGuard>,
    mut documents: ResMut<DocumentState>,
    mut focus: ResMut<InputFocus>,
    mut sources: Query<(Entity, &DocumentSource, &mut EditableText)>,
    mut font: ResMut<FontCx>,
    mut layout: ResMut<LayoutCx>,
) {
    if blocked(&editor, settings.as_deref(), &guard, &mouse) || !documents.visible {
        documents.json.format = None;
        documents.json.ready = None;
        documents.json.locate = None;
        return;
    }
    let Ok((entity, binding, mut text)) = sources.single_mut() else {
        return;
    };
    if documents.json.ready.is_some()
        && let Some(result) = documents.json.ready.take()
    {
        documents.json.format = None;
        let request = result.request.unwrap();
        let doc = documents.current().filter(|doc| {
            request.key.current(doc)
                && doc.mode == ViewMode::Edit
                && binding.key.as_ref() == Some(&doc.source_key())
        });
        if request.source == entity && doc.is_some() {
            if busy(&text)
                || Selection::read(&text) != request.selection
                || focus.get() != Some(entity)
            {
                documents.current_mut().unwrap().status =
                    "输入、光标或焦点已变化，格式化已取消；原草稿保持".into();
            } else {
                match result.formatted {
                    Ok(Some(formatted)) => {
                        let selection = formatted.selection;
                        let doc = documents.current_mut().unwrap();
                        doc.replace_content(
                            formatted.source,
                            formatted.pages,
                            EditContext {
                                before: request.selection,
                                after: selection,
                                ..default()
                            },
                        );
                        doc.status = "已格式化JSON；可一次撤销，保存后写入文件".into();
                        text.editor.set_text(doc.source_text());
                        text.editor
                            .driver(&mut font.context, &mut layout.0)
                            .select_byte_range(selection.anchor, selection.focus);
                        let key = JsonKey::new(doc);
                        documents.json.reveal = Some(Reveal {
                            key,
                            source: entity,
                            selection: text.editor.raw_selection().text_range(),
                        });
                    }
                    Ok(None) => {
                        documents.current_mut().unwrap().status =
                            "JSON已是当前格式；正文保持".into()
                    }
                    Err(error) => documents.current_mut().unwrap().status = error,
                }
            }
        }
    }
    if let Some(key) = documents.json.locate.clone() {
        let doc = documents.current().filter(|doc| {
            key.current(doc)
                && doc.mode == ViewMode::Edit
                && binding.key.as_ref() == Some(&key.document)
        });
        if doc.is_some() && busy(&text) {
            return; // 从预览进入源码，先让原生绑定的TextStart在下一帧采用，再显式定位。
        }
        if let Some(error) = doc
            .and_then(|doc| documents.json.checked(doc))
            .and_then(|checked| checked.error.as_ref())
        {
            let selection = error.selection.clone();
            text.editor
                .driver(&mut font.context, &mut layout.0)
                .select_byte_range(selection.start, selection.end);
            focus.set(entity, FocusCause::Navigated);
            documents.json.reveal = Some(Reveal {
                key,
                source: entity,
                selection,
            });
        }
        documents.json.locate = None;
    }
}

pub(super) fn reveal(
    mut documents: ResMut<DocumentState>,
    mut sources: Query<(
        Entity,
        &DocumentSource,
        &mut EditableText,
        &ComputedNode,
        &TextLayoutInfo,
        &mut DocumentScrollState,
    )>,
) {
    if documents.json.reveal.is_none() {
        return;
    }
    let target = documents.json.reveal.take().unwrap();
    if documents.current().is_none_or(|doc| {
        !documents.visible || doc.mode != ViewMode::Edit || !target.key.current(doc)
    }) {
        return;
    }
    let Ok((entity, binding, mut text, node, layout, mut preservation)) =
        sources.single_mut()
    else {
        return;
    };
    if entity == target.source
        && binding.key.as_ref() == Some(&target.key.document)
        && !busy(&text)
        && text.editor.raw_selection().text_range() == target.selection
    {
        super::super::navigation::reveal_selection(node, layout, &mut text.viewport, &mut preservation);
    }
}
