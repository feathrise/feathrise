//! 子地图数值拖动实时写入整体预览，松手只产生一条引用历史；取消恢复精确起点。

use bevy::prelude::*;
use feathrise_editor_ui::{NumericInputChanged, NumericInputCommitted, NumericInputCommitOutcome,
    NumericInputCommitSource};
use feathrise_map_content::MapInstancePlacement;
use std::sync::Arc;
use crate::{authoring::{EditorAuthoringSession, EditorCommand}, resources::MapEditorState};
use super::{components::Field, resources::{SubMapEditorState, Scrub}, systems};

#[cfg(test)]
#[path = "numeric_preview_tests.rs"]
mod preview_tests;

fn references(session: &EditorAuthoringSession, state: &SubMapEditorState, field: usize, value: f32)
    -> Option<Arc<[MapInstancePlacement]>> {
    if !value.is_finite() { return None; }
    let draft = session.draft.as_ref()?; let id = state.selected.as_ref()?;
    let mut records = draft.manifest.sub_maps.to_vec();
    let record = records.iter_mut().find(|record| &record.id == id)?;
    if field < 3 { record.transform.translation[field] = value; }
    else { record.transform.yaw_degrees = value.rem_euclid(360.0); }
    record.transform.validate().ok()?;
    Some(records.into())
}

pub(super) fn input(mut editor: ResMut<MapEditorState>, mut session: ResMut<EditorAuthoringSession>,
    mut state: ResMut<SubMapEditorState>, fields: Query<&Field>,
    context: crate::viewport::placement_move::MoveInputContext,
    mut changed: MessageReader<NumericInputChanged>, mut committed: MessageReader<NumericInputCommitted>) {
    let usable = systems::enabled(&editor, &session, &state) && context.document_available(&editor)
        && state.selected.as_ref().is_some_and(|id| session.draft.as_ref()
            .is_some_and(|draft| draft.manifest.sub_maps.iter().any(|record| &record.id == id)))
        && state.scrub.as_ref().is_none_or(|transaction| state.selected.as_ref() == Some(&transaction.selected));
    if !usable && let Some(transaction) = state.scrub.take() {
        if session.revision == transaction.expected_revision && session.draft.as_ref().is_some_and(|draft| draft.map_id == transaction.map_id) {
            session.draft.as_mut().unwrap().manifest.sub_maps = transaction.before;
            session.revision = session.revision.wrapping_add(1); editor.dirty = transaction.dirty_before;
        }
    }
    for event in changed.read() {
        let Ok(field) = fields.get(event.entity) else { continue; };
        if !usable { continue; }
        let Some(value) = event.value.as_f32() else { continue; };
        if state.scrub.is_none() {
            let draft = session.draft.as_ref().unwrap();
            state.scrub = Some(Scrub { entity: event.entity, map_id: draft.map_id.clone(),
                selected: state.selected.clone().expect("数值字段有选中的子地图"),
                before: draft.manifest.sub_maps.clone(), expected_revision: session.revision, dirty_before: editor.dirty });
        }
        if state.scrub.as_ref().is_none_or(|transaction| transaction.entity != event.entity || transaction.expected_revision != session.revision) { continue; }
        if let Some(after) = references(&session, &state, field.0, value) {
            if session.draft.as_ref().unwrap().manifest.sub_maps != after {
                session.draft.as_mut().unwrap().manifest.sub_maps = after;
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
                || session.draft.as_ref().is_none_or(|draft| draft.map_id != transaction.map_id) { continue; }
            let after = session.draft.as_ref().unwrap().manifest.sub_maps.clone();
            if event.outcome != NumericInputCommitOutcome::Accepted || after == transaction.before {
                session.draft.as_mut().unwrap().manifest.sub_maps = transaction.before;
                session.revision = session.revision.wrapping_add(1); editor.dirty = transaction.dirty_before;
            } else {
                session.undo.push(EditorCommand::SubMapsChanged { before: transaction.before, after });
                session.redo.clear();
            }
        } else if usable && event.outcome == NumericInputCommitOutcome::Accepted && event.edited {
            if let Some(value) = event.value.as_f32() && let Some(after) = references(&session, &state, field.0, value) {
                systems::apply(&mut session, &mut editor, after);
            }
        }
    }
}
