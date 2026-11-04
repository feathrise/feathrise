//! 查找/替换快捷键、中文输入仲裁与冻结选区；替换只提交完整身份的后台请求。

use super::super::{
    components::DocumentSource, history::Selection, resources::DocumentState, types::ViewMode,
};
use super::{components::*, index, resources::DocumentNavigation, types::*};
use crate::resources::{
    EditorTextInputGuard, MapEditorNumericScrubMouseGuard, MapEditorSettingsState, MapEditorState,
};
use bevy::{
    input_focus::{FocusCause, InputFocus},
    prelude::*,
    text::{EditableText, FontCx, LayoutCx, TextEdit},
};

pub(super) fn input(
    editor: Res<MapEditorState>,
    settings: Option<Res<MapEditorSettingsState>>,
    guard: Res<EditorTextInputGuard>,
    mouse: Res<MapEditorNumericScrubMouseGuard>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut documents: ResMut<DocumentState>,
    mut state: ResMut<DocumentNavigation>,
    mut focus: ResMut<InputFocus>,
    sources: Query<(Entity, &DocumentSource, &EditableText)>,
    mut fields: Query<(Entity, &NavigationField, &mut EditableText), Without<DocumentSource>>,
    buttons: Query<(&feathrise_editor_ui::EditorButtonClick, &NavigationAction), Changed<feathrise_editor_ui::EditorButtonClick>>,
    editor_inputs: Query<(), With<crate::ui::text_input::EditorTextInput>>,
) {
    if !editor.open
        || editor.close_requested
        || editor.new_map_dialog_open
        || settings.is_some_and(|settings| settings.open)
        || mouse.blocked()
        || !documents.visible
    {
        state.pending = None; // 隐藏或模态后的旧请求不能在恢复时补放。
        state.cancel_replace();
        return;
    }
    let Some(doc) = documents.current() else {
        return;
    };
    if state
        .document
        .as_ref()
        .is_some_and(|key| key.path != doc.path || key.generation != doc.generation)
    {
        return; // Tasks随后切换会话，当前帧不能把旧字段输入交给新文件。
    }
    if documents.pending_load.is_some() || (documents.io.is_some() && documents.saving.is_none()) {
        state.cancel_replace();
        return; // 重新载入不得重新编辑即将丢弃的旧正文。
    }
    if state
        .scope
        .as_ref()
        .is_some_and(|scope| scope.document != doc.source_key() || scope.revision != doc.revision)
    {
        state.scope = None;
        state.invalidate();
        state.hint = "正文已变化，请重新选择范围或关闭“仅选区”".into();
    }
    let own_focus = focus
        .get()
        .is_some_and(|entity| sources.contains(entity) || fields.contains(entity));
    if focus
        .get()
        .is_some_and(|entity| editor_inputs.contains(entity))
        && !own_focus
    {
        return;
    }
    // 只收已确认文字，候选输入不进入查询，也不把IME的Enter/Esc当成导航。
    for (_, field, text) in &mut fields {
        let value = text.value().to_string(); // 字段最多128字符，正文Arc由后台共享。
        if *field == NavigationField::Query && state.mode.searching() && value != state.query {
            state.query = value;
            state.invalidate();
            state.hint.clear();
        } else if *field == NavigationField::Replacement
            && state.mode == NavigationMode::Replace
            && value != state.replacement
        {
            state.replacement = value;
            state.cancel_replace();
            state.hint.clear();
        } else if *field == NavigationField::Line
            && state.mode == NavigationMode::Go
            && value != state.line
        {
            state.line = value;
            state.hint.clear();
        }
    }
    if guard.block_shortcuts
        || sources.iter().any(|(_, _, text)| busy(text))
        || fields.iter().any(|(_, _, text)| busy(text))
    {
        state.cancel_replace();
        return;
    }
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let mut actions: Vec<_> = buttons
        .iter()
        .filter(|(pressed, _)| pressed.is_clicked())
        .map(|(_, action)| *action)
        .collect();
    if ctrl && keys.just_pressed(KeyCode::KeyF) {
        actions.push(NavigationAction::Find);
        keys.clear_just_pressed(KeyCode::KeyF);
    }
    if ctrl && keys.just_pressed(KeyCode::KeyG) {
        actions.push(NavigationAction::Go);
        keys.clear_just_pressed(KeyCode::KeyG);
    }
    if ctrl && keys.just_pressed(KeyCode::KeyH) {
        actions.push(NavigationAction::Replace);
        keys.clear_just_pressed(KeyCode::KeyH);
    }
    if state.mode != NavigationMode::Closed && own_focus && keys.just_pressed(KeyCode::Escape) {
        actions.push(NavigationAction::Close);
        keys.clear_just_pressed(KeyCode::Escape);
    }
    if own_focus && keys.just_pressed(KeyCode::Enter) {
        let field = focus
            .get()
            .and_then(|entity| fields.get(entity).ok())
            .map(|(_, field, _)| *field);
        let action = match field {
            Some(NavigationField::Query) if state.mode.searching() => Some(
                if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) {
                    NavigationAction::Previous
                } else {
                    NavigationAction::Next
                },
            ),
            Some(NavigationField::Replacement) if state.mode == NavigationMode::Replace => {
                Some(NavigationAction::ReplaceCurrent)
            }
            Some(NavigationField::Line) if state.mode == NavigationMode::Go => {
                Some(NavigationAction::SubmitLine)
            }
            _ => None,
        };
        if let Some(action) = action {
            actions.push(action);
            keys.clear_just_pressed(KeyCode::Enter);
        }
    }
    for action in actions {
        match action {
            NavigationAction::Find | NavigationAction::Replace | NavigationAction::Go => {
                if let Some(doc) = documents.current_mut() {
                    doc.mode = ViewMode::Edit;
                }
                state.mode = match action {
                    NavigationAction::Find => NavigationMode::Find,
                    NavigationAction::Replace => NavigationMode::Replace,
                    _ => NavigationMode::Go,
                };
                state.cancel_replace();
                state.hint.clear();
                state.pending = None;
                let target = if state.mode.searching() {
                    NavigationField::Query
                } else {
                    NavigationField::Line
                };
                for (entity, field, mut text) in &mut fields {
                    if *field == target {
                        let value = if target == NavigationField::Query {
                            &state.query
                        } else {
                            &state.line
                        };
                        if text.value().to_string() != *value {
                            text.clear();
                            text.editor.set_text(value);
                        }
                        text.queue_edit(TextEdit::SelectAll);
                        focus.set(entity, FocusCause::Navigated);
                    }
                }
            }
            NavigationAction::Close => {
                state.mode = NavigationMode::Closed;
                state.pending = None;
                state.cancel_replace();
                if let Ok((entity, _, _)) = sources.single() {
                    focus.set(entity, FocusCause::Navigated);
                }
            }
            NavigationAction::Previous | NavigationAction::Next if state.mode.searching() => {
                state.cancel_replace();
                state.pending = Some(NavigationRequest::Next(
                    action == NavigationAction::Previous,
                ));
            }
            NavigationAction::CaseSensitive | NavigationAction::WholeWord
                if state.mode.searching() =>
            {
                if action == NavigationAction::CaseSensitive {
                    state.options.case_sensitive = !state.options.case_sensitive;
                } else {
                    state.options.whole_word = !state.options.whole_word;
                }
                state.invalidate();
                state.hint.clear();
            }
            NavigationAction::InSelection if state.mode.searching() => {
                if state.in_selection {
                    state.in_selection = false;
                    state.scope = None;
                } else if let Ok((_, binding, text)) = sources.single()
                    && binding.key.as_ref() == Some(&documents.current().unwrap().source_key())
                    && !text.editor.raw_selection().text_range().is_empty()
                {
                    let doc = documents.current().unwrap();
                    state.in_selection = true;
                    state.scope = Some(SelectionScope {
                        document: doc.source_key(),
                        revision: doc.revision,
                        range: text.editor.raw_selection().text_range(),
                    });
                } else {
                    state.hint = "请先在正文选中文字，再开启“仅选区”".into();
                    continue;
                }
                state.invalidate();
                state.hint.clear();
            }
            NavigationAction::ReplaceCurrent | NavigationAction::ReplaceAll
                if state.mode == NavigationMode::Replace =>
            {
                if state.replace.is_some() || state.ready.is_some() {
                    continue;
                }
                let doc = documents.current().unwrap();
                let Some(index) = state
                    .index
                    .as_ref()
                    .filter(|found| {
                        index::current(&found.key, doc, &state)
                            && found.error.is_none()
                            && !found.matches.is_empty()
                    })
                    .cloned()
                else {
                    state.hint = "没有可替换的当前匹配；请等待查找完成或调整范围".into();
                    continue;
                };
                let Ok((entity, binding, text)) = sources.single() else {
                    continue;
                };
                if doc.mode != ViewMode::Edit || binding.key.as_ref() != Some(&doc.source_key()) {
                    continue;
                }
                let selection = Selection::read(text);
                let range = text.editor.raw_selection().text_range();
                let target = if action == NavigationAction::ReplaceCurrent {
                    let selected = index
                        .matches
                        .binary_search_by(|found| found.start.cmp(&range.start))
                        .ok()
                        .filter(|selected| index.matches[*selected] == range)
                        .or_else(|| index::next_match(&index.matches, None, selection.focus, false))
                        .unwrap();
                    Some(index.matches[selected].clone())
                } else {
                    None
                };
                // 按钮与替换框Enter共用事务；保留替换框焦点以便重复操作，正文依然只有一个输入。
                if let Some((entity, _, _)) = fields
                    .iter_mut()
                    .find(|(_, field, _)| **field == NavigationField::Replacement)
                {
                    focus.set(entity, FocusCause::Navigated);
                }
                state.operation = state.operation.wrapping_add(1);
                state.replace = Some(ReplacementRequest {
                    index,
                    ticket: state.operation,
                    source: entity,
                    focus: focus.get(),
                    selection,
                    replacement: state.replacement.clone(),
                    target,
                });
                state.pending = None;
                state.hint = "正在后台准备完整替换…".into();
                let doc = documents.current_mut().unwrap();
                doc.history.break_group();
                doc.confirm_reload = false;
            }
            NavigationAction::SubmitLine if state.mode == NavigationMode::Go => {
                match state.line.trim().parse::<usize>() {
                    Ok(line) if line > 0 => {
                        state.pending = Some(NavigationRequest::Line(line));
                    }
                    _ => {
                        state.hint = "请输入从1开始的行号".into();
                    }
                }
            }
            _ => (),
        }
    }
}

pub(super) fn busy(text: &EditableText) -> bool {
    text.is_composing() || text.pending_paste.is_some() || !text.pending_edits.is_empty()
}

pub(super) fn navigate(
    editor: Res<MapEditorState>,
    settings: Option<Res<MapEditorSettingsState>>,
    guard: Res<EditorTextInputGuard>,
    mouse: Res<MapEditorNumericScrubMouseGuard>,
    fields: Query<&EditableText, (With<NavigationField>, Without<DocumentSource>)>,
    documents: Res<DocumentState>,
    mut state: ResMut<DocumentNavigation>,
    mut source: Query<(Entity, &DocumentSource, &mut EditableText)>,
    mut font: ResMut<FontCx>,
    mut layout: ResMut<LayoutCx>,
    mut focus: ResMut<InputFocus>,
) {
    if !editor.open
        || editor.close_requested
        || editor.new_map_dialog_open
        || settings.is_some_and(|settings| settings.open)
        || guard.block_shortcuts
        || mouse.blocked()
        || fields.iter().any(busy)
    {
        state.pending = None; // 新模态、手势或中文候选接管后取消旧导航。
        return;
    }
    let Some(request) = state.pending else {
        return;
    };
    let Some(doc) = documents
        .current()
        .filter(|doc| documents.visible && doc.mode == ViewMode::Edit)
    else {
        return;
    };
    let Some(index) = state
        .index
        .as_ref()
        .filter(|index| index::current(&index.key, doc, &state))
    else {
        return;
    };
    let Ok((entity, binding, mut text)) = source.single_mut() else {
        return;
    };
    if binding.key.as_ref() != Some(&index.key.document) || busy(&text) {
        return;
    }
    let target_key = index.key.clone();
    let mut driver = text.editor.driver(&mut font.context, &mut layout.0);
    let mut reveal = false;
    match request {
        NavigationRequest::Next(back) => {
            let caret = driver.editor.raw_selection().focus().index();
            let selected = state.selected.filter(|selected| {
                index
                    .matches
                    .get(*selected)
                    .is_some_and(|range| *range == driver.editor.raw_selection().text_range())
            });
            let selected = index::next_match(&index.matches, selected, caret, back);
            if let Some(selected) = selected {
                let range = index.matches[selected].clone();
                driver.select_byte_range(range.start, range.end);
                reveal = true;
                state.selected = Some(selected);
            }
            // 查找框保留焦点，可连续Enter/Shift+Enter；选区仍在唯一源码输入中。
        }
        NavigationRequest::Line(line) => {
            if let Some(byte) = index.lines.get(line - 1).copied() {
                driver.move_to_byte(byte);
                reveal = true;
                state.mode = NavigationMode::Closed;
                state.hint.clear();
                focus.set(entity, FocusCause::Navigated);
            } else {
                state.hint = format!("行号范围：1–{}", index.lines.len());
            }
        }
    }
    if reveal {
        state.reveal = Some(RevealTarget {
            source: entity,
            key: target_key,
            selection: driver.editor.raw_selection().text_range(),
        });
    }
    state.pending = None;
}
