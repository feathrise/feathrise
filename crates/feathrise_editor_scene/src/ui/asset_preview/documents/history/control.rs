//! 在原生队列采用前记录事务边界，采用后只处理当前文件的显式撤销/重做。

use super::super::{components::DocumentSource, resources::DocumentState, types::ViewMode};
use super::{components::HistoryAction, resources::HistoryCapture, types::*};
use crate::resources::{
    EditorTextInputGuard, SceneEditorNumericScrubMouseGuard, SceneEditorSettingsState, SceneEditorState,
};
use bevy::{
    input_focus::{FocusCause, InputFocus},
    prelude::*,
    text::{EditableText, FontCx, LayoutCx, TextEdit},
};
use std::time::Instant;

pub(super) fn prepare(
    mut documents: ResMut<DocumentState>,
    mut capture: ResMut<HistoryCapture>,
    focus: Option<Res<InputFocus>>,
    sources: Query<(Entity, &DocumentSource, &EditableText)>,
) {
    let Ok((entity, binding, text)) = sources.single() else {
        return;
    };
    let Some(key) = binding.key.as_ref().filter(|key| {
        documents
            .current()
            .is_some_and(|doc| doc.mode == ViewMode::Edit && doc.source_key() == **key)
    }) else {
        *capture = HistoryCapture::default();
        return;
    };
    let changed_file = capture.key.as_ref() != Some(key);
    if changed_file {
        *capture = HistoryCapture {
            key: Some(key.clone()),
            ..default()
        };
    }
    let selection = Selection::read(text);
    let ime_action = text.pending_edits.iter().any(|edit| {
        matches!(
            edit,
            TextEdit::ImeSetCompose { .. } | TextEdit::ImeCommit { .. }
        )
    });
    if !text.is_composing() && ime_action && capture.ime_before.is_none() {
        capture.ime_before = Some(selection); // 候选替换选区前冻结；预编辑文字不产生历史步骤。
    } else if !text.is_composing() && !ime_action {
        capture.ime_before = None;
    }
    let typing = !text.is_composing()
        && text.pending_paste.is_none()
        && selection.anchor == selection.focus
        && !text.pending_edits.is_empty()
        && text.pending_edits.len() <= 32
        && text.pending_edits.iter().all(|edit| {
            matches!(edit, TextEdit::Insert(value)
            if value.chars().next().is_some_and(|ch| !ch.is_control()) && value.chars().nth(1).is_none())
        });
    capture.before = capture.ime_before.unwrap_or(selection);
    capture.typing = typing;
    capture.at = Some(Instant::now());
    if changed_file
        || focus
            .as_ref()
            .is_none_or(|focus| focus.get() != Some(entity))
        || (!typing
            && (!text.pending_edits.is_empty()
                || text.is_composing()
                || text.pending_paste.is_some()))
    {
        if documents
            .current()
            .is_some_and(|doc| doc.history.group.is_some())
        {
            documents.current_mut().unwrap().history.break_group(); // 光标移动、粘贴、删除、保存和切文件都划分输入步骤。
        }
    }
}

pub(super) fn input(
    editor: Res<SceneEditorState>,
    settings: Option<Res<SceneEditorSettingsState>>,
    guard: Res<EditorTextInputGuard>,
    mouse: Res<SceneEditorNumericScrubMouseGuard>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut focus: ResMut<InputFocus>,
    mut documents: ResMut<DocumentState>,
    mut sources: Query<(Entity, &DocumentSource, &mut EditableText)>,
    buttons: Query<(&HistoryAction, &feathrise_editor_ui::EditorButtonClick), Changed<feathrise_editor_ui::EditorButtonClick>>,
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
    {
        return;
    }
    let Ok((entity, binding, mut text)) = sources.single_mut() else {
        return;
    };
    let Some(doc) = documents.current().filter(|doc| {
        doc.mode == ViewMode::Edit && binding.key.as_ref() == Some(&doc.source_key())
    }) else {
        return;
    };
    if text.is_composing() || text.pending_paste.is_some() || !text.pending_edits.is_empty() {
        return;
    }
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let mut action = buttons
        .iter()
        .find(|(_, interaction)| interaction.is_clicked())
        .map(|(action, _)| *action);
    // 查找框、预设字段和场景历史不接收源码撤销；只在正文拥有焦点时消费快捷键。
    if focus.get() == Some(entity) && ctrl {
        if keys.just_pressed(KeyCode::KeyZ) {
            action = Some(if shift {
                HistoryAction::Redo
            } else {
                HistoryAction::Undo
            });
            keys.clear_just_pressed(KeyCode::KeyZ);
        } else if keys.just_pressed(KeyCode::KeyY) {
            action = Some(HistoryAction::Redo);
            keys.clear_just_pressed(KeyCode::KeyY);
        }
    }
    let Some(action) = action else {
        return;
    };
    let available = if action == HistoryAction::Redo {
        doc.history.can_redo()
    } else {
        doc.history.can_undo()
    };
    if !available {
        return;
    }
    let doc = documents.current_mut().unwrap();
    let selection = doc.travel(action == HistoryAction::Redo).unwrap();
    text.editor.set_text(doc.source_text());
    // 业务快照保留UTF-8字节选区方向；官方driver一次重排并恢复光标，原实体/样式/视区保持。
    text.editor
        .driver(&mut font.context, &mut layout.0)
        .select_byte_range(selection.anchor, selection.focus);
    focus.set(entity, FocusCause::Navigated);
}
