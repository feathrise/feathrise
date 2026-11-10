//! 子场景数值拖动实时写入整体预览，松手只产生一条引用历史；取消恢复精确起点。

use bevy::prelude::*;
use feathrise_editor_ui::{NumericInputChanged, NumericInputCommitted, NumericInputCommitOutcome,
    NumericInputCommitSource};
use feathrise_scene_content::SceneInstancePlacement;
use std::sync::Arc;
use crate::{authoring::{EditorAuthoringSession, EditorCommand}, resources::SceneEditorState};
use super::{components::Field, resources::{SubsceneEditorState, Scrub}, systems};

#[cfg(test)]
#[path = "numeric_preview_tests.rs"]
mod preview_tests;

fn references(session: &EditorAuthoringSession, state: &SubsceneEditorState, field: usize, value: f32)
    -> Option<Arc<[SceneInstancePlacement]>> {
    if !value.is_finite() { return None; }
    let draft = session.draft.as_ref()?; let id = state.selected.as_ref()?;
    let mut records = draft.manifest.subscenes.to_vec();
    let record = records.iter_mut().find(|record| &record.id == id)?;
    if field < 3 { record.transform.translation[field] = value; }
    else { record.transform.yaw_degrees = value.rem_euclid(360.0); }
    record.transform.validate().ok()?;
    Some(records.into())
}

pub(super) fn input(mut editor: ResMut<SceneEditorState>, mut session: ResMut<EditorAuthoringSession>,
    mut state: ResMut<SubsceneEditorState>, fields: Query<&Field>,
    context: crate::viewport::placement_move::MoveInputContext,
    mut changed: MessageReader<NumericInputChanged>, mut committed: MessageReader<NumericInputCommitted>) {
    let usable = systems::enabled(&editor, &session, &state) && context.document_available(&editor)
        && state.selected.as_ref().is_some_and(|id| session.draft.as_ref()
            .is_some_and(|draft| draft.manifest.subscenes.iter().any(|record| &record.id == id)))
        && state.scrub.as_ref().is_none_or(|transaction| state.selected.as_ref() == Some(&transaction.selected));
    if !usable && let Some(transaction) = state.scrub.take() {
        if session.revision == transaction.expected_revision && session.draft.as_ref().is_some_and(|draft| draft.scene_id == transaction.scene_id) {
            session.draft.as_mut().unwrap().manifest.subscenes = transaction.before;
            session.revision = session.revision.wrapping_add(1); editor.dirty = transaction.dirty_before;
        }
    }
    for event in changed.read() {
        let Ok(field) = fields.get(event.entity) else { continue; };
        if !usable { continue; }
        let Some(value) = event.value.as_f32() else { continue; };
        if state.scrub.is_none() {
            let draft = session.draft.as_ref().unwrap();
            state.scrub = Some(Scrub { entity: event.entity, scene_id: draft.scene_id.clone(),
                selected: state.selected.clone().expect("数值字段有选中的子场景"),
                before: draft.manifest.subscenes.clone(), expected_revision: session.revision, dirty_before: editor.dirty });
        }
        if state.scrub.as_ref().is_none_or(|transaction| transaction.entity != event.entity || transaction.expected_revision != session.revision) { continue; }
        if let Some(after) = references(&session, &state, field.0, value) {
            if session.draft.as_ref().unwrap().manifest.subscenes != after {
                session.draft.as_mut().unwrap().manifest.subscenes = after;
                session.mark_changed(&mut editor);
                state.scrub.as_mut().unwrap().expected_revision = session.revision;
            }
        }
    }
    for event in committed.read() {
        let Ok(field) = fields.get(event.entity) else { continue; };
        if event.source == NumericInputCommitSource::Scrub {
            let Some(transaction) = state.scrub.take() else { continue; };
            if transaction.entity != event.entity || transaction.expected_revision != session.revision
                || session.draft.as_ref().is_none_or(|draft| draft.scene_id != transaction.scene_id) { continue; }
            let after = session.draft.as_ref().unwrap().manifest.subscenes.clone();
            if event.outcome != NumericInputCommitOutcome::Accepted || after == transaction.before {
                session.draft.as_mut().unwrap().manifest.subscenes = transaction.before;
                session.revision = session.revision.wrapping_add(1); editor.dirty = transaction.dirty_before;
            } else {
                session.undo.push(EditorCommand::SubscenesChanged { before: transaction.before, after });
                session.redo.clear();
            }
        } else if usable && event.outcome == NumericInputCommitOutcome::Accepted && event.edited {
            if let Some(value) = event.value.as_f32() && let Some(after) = references(&session, &state, field.0, value) {
                systems::apply(&mut session, &mut editor, after);
            }
        }
    }
}
