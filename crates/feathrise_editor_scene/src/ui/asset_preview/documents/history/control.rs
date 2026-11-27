//! 在原生队列采用前记录事务边界，采用后只处理当前文件的显式撤销/重做。

use super::super::{components::{DocumentSource, SharedDocumentTarget}, resources::DocumentState, types::ViewMode};
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
            .is_some_and(|doc| documents.editing() && doc.source_key() == **key)
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
    fields: Query<&EditableText, (With<super::super::ui_layout::Field>, Without<DocumentSource>)>,
    buttons: Query<(&HistoryAction, &feathrise_editor_ui::EditorButtonClick), Changed<feathrise_editor_ui::EditorButtonClick>>,
    shared_buttons: Query<(&crate::components::SceneEditorActionButton, &feathrise_editor_ui::EditorButtonClick,
        &SharedDocumentTarget, Has<bevy::ui::InteractionDisabled>), Changed<feathrise_editor_ui::EditorButtonClick>>,
    inputs: Query<(), Or<(With<EditableText>, With<crate::components::EditorTextInput>)>>,
    ui_layouts: Option<Res<super::super::ui_layout::UiLayouts>>,
    workspace: Option<Res<crate::ui::vegetation_preset_editor::VegetationPresetEditorState>>,
    mut font: ResMut<FontCx>,
    mut layout: ResMut<LayoutCx>,
) {
    if !editor.open
        || editor.close_requested
        || editor.new_scene_dialog_open
        || editor.scene_transition_in_progress
        || editor.pending_scene_transition.is_some()
        || editor.scene_switch_confirmation.is_some()
        || settings.is_some_and(|settings| settings.open || settings.owns_mouse_this_frame)
        || guard.block_shortcuts
        || mouse.blocked()
        || !documents.visible
    {
        return;
    }
    let Some(doc) = documents.current() else {
        return;
    };
    let ui_document = documents.ui_layout_in_center()
        && workspace.as_ref().is_none_or(|workspace| {
            workspace.active_tab == crate::ui::vegetation_preset_editor::PresetWorkspaceTab::Document
        });
    let source_entity = sources.iter().find_map(|(entity, binding, _)| {
        (documents.editing() && binding.key.as_ref() == Some(&doc.source_key())).then_some(entity)
    });
    let canvas = ui_document && doc.mode == ViewMode::Preview;
    if source_entity.is_none() && !canvas { return; }
    if sources.iter().any(|(_, _, text)| super::super::ui_layout::input_pending(text))
        || fields.iter().any(super::super::ui_layout::input_pending)
    {
        return;
    }
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let mut action = buttons
        .iter()
        .find(|(_, interaction)| source_entity.is_some() && interaction.is_clicked())
        .map(|(action, _)| *action);
    if ui_document {
        action = shared_buttons.iter().find_map(|(button, click, target, disabled)| {
            if disabled || !click.is_clicked() || !target.matches(&documents) { return None; }
            match button.0 {
                crate::types::EditorAction::Undo => Some(HistoryAction::Undo),
                crate::types::EditorAction::Redo => Some(HistoryAction::Redo),
                _ => None,
            }
        }).or(action);
    }
    // 画布沿用原文件历史；属性、查找及其他文本框仍独占自己的撤销快捷键。
    let owns_shortcuts = source_entity.is_some_and(|entity| focus.get() == Some(entity))
        || (canvas && focus.get().is_none_or(|entity| !inputs.contains(entity)));
    if owns_shortcuts && ctrl {
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
    if ui_document && documents.ui_layout_command_status(ui_layouts.as_deref()).is_none_or(|status| {
        status.reason(if action == HistoryAction::Redo {
            crate::types::EditorAction::Redo
        } else { crate::types::EditorAction::Undo }).is_some()
    }) {
        return; // 后台属性变更采用前不能越过它撤销同一文档。
    }
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
    let Some(entity) = source_entity else { return; }; // 画布由新修订派生，不另造正文输入。
    let Ok((_, _, mut text)) = sources.get_mut(entity) else { return; };
    text.editor.set_text(doc.source_text());
    // 业务快照保留UTF-8字节选区方向；官方driver一次重排并恢复光标，原实体/样式/视区保持。
    text.editor
        .driver(&mut font.context, &mut layout.0)
        .select_byte_range(selection.anchor, selection.focus);
    focus.set(entity, FocusCause::Navigated);
}
