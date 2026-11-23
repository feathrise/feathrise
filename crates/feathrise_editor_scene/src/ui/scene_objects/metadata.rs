//! 场景对象元数据整批发布、历史预检和有界写回。摆放文件仍使用原记录格式。

use crate::{
    authoring::{EditorAuthoringSession, EditorCommand, SceneObjectBatchChange},
    resources::SceneEditorState,
    ui::gltf_browser::EditorGltfBrowserState,
};
use crate::authoring::placement_deletion::PlacementDeletionIdentity;
use crate::authoring::placement_participation;
use feathrise_scene_content::{SceneId, ScenePlacementEditorMetadata, ScenePlacementRecord};
use std::{collections::BTreeMap, sync::Arc};

pub(super) const METADATA_PER_FRAME: usize = 128;
pub(super) type MetadataBatch = BTreeMap<String, (usize, Arc<ScenePlacementEditorMetadata>)>;

pub(super) struct PendingMetadataWrite {
    scene: SceneId,
    document: PlacementDeletionIdentity,
    changes: MetadataBatch,
}

pub(super) struct MetadataHistory {
    scene: SceneId,
    document: PlacementDeletionIdentity,
    revision: u64,
    structure: u64,
    undoing: bool,
    command: usize,
    cursor: usize,
    changes: MetadataBatch,
}

pub(crate) fn effective_metadata<'a>(
    state: Option<&'a super::SceneObjectState>,
    session: &EditorAuthoringSession,
    record: &'a ScenePlacementRecord,
) -> &'a Arc<ScenePlacementEditorMetadata> {
    state.and_then(|state| state.metadata_write.as_ref())
        .and_then(|write| {
            let draft = session.draft.as_ref().filter(|draft| draft.scene_id == write.scene
                && write.document.matches(&session.placement_deletions))?;
            let (index, value) = write.changes.get(&record.id)?;
            // 损坏来源可能有重名ID；覆盖只作用于已验证的记录槽位。
            draft.placements.placements.get(*index)
                .is_some_and(|source| std::ptr::eq(source, record)).then_some(value)
        })
        .unwrap_or(&record.editor)
}

pub(crate) fn effective_participation(
    state: Option<&super::SceneObjectState>,
    session: &EditorAuthoringSession,
    record: &ScenePlacementRecord,
) -> bool {
    crate::authoring::placement_participation::participates(effective_metadata(state, session, record))
}

impl super::SceneObjectState {
    pub(super) fn publish_metadata(&mut self, session: &EditorAuthoringSession, changes: MetadataBatch) {
        debug_assert!(self.metadata_write.is_none());
        if !changes.is_empty() {
            self.metadata_write = Some(PendingMetadataWrite {
                scene: session.draft.as_ref().expect("发布元数据前已检查草稿").scene_id.clone(),
                document: session.placement_deletions.identity(),
                changes,
            });
        }
    }

    pub(super) fn write_metadata(
        &mut self,
        session: &mut EditorAuthoringSession,
        browser: &EditorGltfBrowserState,
        mut previews: Option<(&mut super::participation_previews::ParticipationPreviews<'_, '_>, &mut usize)>,
    ) {
        let Some(write) = self.metadata_write.as_mut() else { return; };
        let Some(draft) = session.draft.as_mut().filter(|draft| draft.scene_id == write.scene
            && write.document.matches(&session.placement_deletions)) else {
            if self.retirement.is_none() {
                let discarded = self.metadata_write.take();
                super::systems::retire(self, discarded);
            }
            return;
        };
        // 完整新值已经对读取方生效；每帧只转移固定数量的 Arc。移出覆盖时，记录已接管相同值。
        for _ in 0..METADATA_PER_FRAME {
            let Some((id, (index, value))) = write.changes.first_key_value() else { break; };
            let index = if draft.placements.placements.get(*index).is_some_and(|record| record.id == *id) {
                Some(*index)
            } else if browser.placement_index_ready(session.placement_structure_revision) {
                browser.placement_record_index(id, session.placement_structure_revision)
            } else {
                break; // 来源重排时等待可证明身份的完整索引，不能套用旧下标。
            };
            if let Some((context, remaining)) = previews.as_mut()
                && let Some(record) = index.and_then(|index| draft.placements.placements.get(index)).filter(|record| record.id == *id)
            {
                let enabled = placement_participation::participates(value);
                if enabled != placement_participation::participates(&record.editor)
                    && !context.apply(draft, record, enabled, &mut session.placement_previews, remaining)
                {
                    break; // 作者根受独立预算约束，当前覆盖继续生效，下一帧接续写回。
                }
            }
            let (id, (_, value)) = write.changes.pop_first().expect("已经取得首项");
            if let Some(record) = index.and_then(|index| draft.placements.placements.get_mut(index)).filter(|record| record.id == id) {
                record.editor = value;
            }
        }
        if write.changes.is_empty() { self.metadata_write = None; }
    }

    pub(super) fn check_metadata_history(&mut self, session: &EditorAuthoringSession, editor: &mut SceneEditorState) {
        let Some(capture) = self.history.as_ref() else { return; };
        let requested = if capture.undoing { editor.undo_requested } else { editor.redo_requested && !editor.undo_requested };
        let top = if capture.undoing { session.undo.last() } else { session.redo.last() };
        let same_command = top.is_some_and(|command| matches!(command.unscoped(),
            EditorCommand::SceneObjectsChanged(change) if change.as_ref() as *const _ as usize == capture.command));
        if requested && accepts_history(editor)
            && editor.selected_scene_id.as_ref() == Some(&capture.scene)
            && capture.document.matches(&session.placement_deletions)
            && session.draft.as_ref().is_some_and(|draft| draft.scene_id == capture.scene)
            && session.revision == capture.revision && session.placement_structure_revision == capture.structure
            && same_command
        { return; }
        if requested && accepts_history(editor) {
            editor.status = "场景对象历史来源已变化，原历史记录已保留".into();
        }
        if capture.undoing { editor.undo_requested = false; } else { editor.redo_requested = false; }
        let discarded = self.history.take();
        super::systems::retire(self, discarded);
    }

    pub(crate) fn apply_metadata_history(
        &mut self,
        session: &mut EditorAuthoringSession,
        editor: &mut SceneEditorState,
        undoing: bool,
    ) {
        if !accepts_history(editor) || self.managing() || self.retirement.is_some() { return; }
        let Some(draft) = session.draft.as_ref() else { return; };
        if editor.selected_scene_id.as_ref() != Some(&draft.scene_id) { return; }
        let command = if undoing { session.undo.last() } else { session.redo.last() };
        let Some(EditorCommand::SceneObjectsChanged(command)) = command.map(EditorCommand::unscoped) else { return; };
        let prepared = self.prepare_metadata_history(&draft.scene_id, session, command, undoing).and_then(|ready| {
            if ready && let Some(rename) = &command.folder {
                rename.apply(&mut session.draft.as_mut().unwrap().object_folders, undoing)?;
            }
            Ok(ready)
        });
        match prepared {
            Ok(false) => editor.status = "正在准备场景对象撤销或重做".into(),
            Ok(true) => {
                let capture = self.history.take().expect("已完成对象历史候选");
                if undoing {
                    editor.undo_requested = false;
                    let command = session.pop_undo().expect("已取得撤销命令");
                    session.redo.push(command);
                    editor.status = "已撤销上一项作者操作".into();
                } else {
                    editor.redo_requested = false;
                    let command = session.redo.pop().expect("已取得重做命令");
                    let mut retired = Vec::new();
                    session.push_bounded_history_retiring(command, &mut retired);
                    if !retired.is_empty() { super::systems::retire(self, retired); }
                    editor.status = "已重做作者操作".into();
                }
                self.publish_metadata(session, capture.changes);
                session.mark_scene_objects_changed(editor);
            }
            Err(error) => {
                if undoing { editor.undo_requested = false; } else { editor.redo_requested = false; }
                editor.status = error;
                let discarded = self.history.take();
                super::systems::retire(self, discarded);
            }
        }
    }

    fn prepare_metadata_history(
        &mut self,
        scene: &SceneId,
        session: &EditorAuthoringSession,
        command: &SceneObjectBatchChange,
        undoing: bool,
    ) -> Result<bool, String> {
        let capture = self.history.get_or_insert_with(|| MetadataHistory {
            scene: scene.clone(), document: session.placement_deletions.identity(),
            revision: session.revision, structure: session.placement_structure_revision,
            undoing, command: command as *const _ as usize, cursor: 0, changes: BTreeMap::new(),
        });
        let records = &session.draft.as_ref().expect("已检查草稿").placements.placements;
        let end = (capture.cursor + METADATA_PER_FRAME).min(command.changes.len());
        for change in &command.changes[capture.cursor..end] {
            let record = records.get(change.index).filter(|record| record.id == change.id)
                .ok_or("场景对象历史目标已经变化")?;
            let (before, after) = if undoing { (&change.after, &change.before) } else { (&change.before, &change.after) };
            if &record.editor != before { return Err("场景对象历史版本已经变化".into()); }
            after.validate()?;
            if capture.changes.insert(change.id.clone(), (change.index, after.clone())).is_some() {
                return Err(format!("场景对象历史标识重复：{}", change.id));
            }
        }
        capture.cursor = end;
        Ok(end == command.changes.len())
    }
}

fn accepts_history(editor: &SceneEditorState) -> bool {
    editor.open && !editor.close_requested && !editor.scene_transition_in_progress
        && editor.pending_scene_transition.is_none() && editor.scene_switch_confirmation.is_none()
}
