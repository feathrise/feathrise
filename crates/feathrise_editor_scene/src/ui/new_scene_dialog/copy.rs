//! 整场景复制沿用资料模态和统一保存，等待源修订的成功回执后才交给后台事务。

use bevy::{prelude::*, tasks::IoTaskPool};
use feathrise_scene_content::{SceneCatalogEntry, SceneId};

use crate::authoring::{
    EditorAuthoringSession, creation_input::{normalize_display_name, scene_directory_name},
    scene_copy::copy_scene, workspace_asset_root,
};
use crate::resources::{
    SceneCopyAfterSave, SceneCopyControl, SceneCopySaveStage, SceneCreationResult, SceneCreationTask,
    SceneEditorState,
};
use crate::types::{EditorStatusKind, EditorStatusOperation, EditorStatusTarget, SceneCopyTarget};
use crate::ui::scene_documents::SceneDocuments;

pub(crate) fn open(
    editor: &mut SceneEditorState,
    session: &EditorAuthoringSession,
    documents: Option<&SceneDocuments>,
    entry: &SceneCatalogEntry,
) {
    let dirty = source_dirty(editor, session, documents, &entry.id);
    editor.new_scene_name = format!("{} 副本", entry.display_name);
    editor.new_scene_notes.clear();
    editor.new_scene_edit_target = None;
    editor.new_scene_copy_target = Some(SceneCopyTarget {
        scene_id: entry.id.clone(), display_name: entry.display_name.clone(), dirty,
    });
    editor.new_scene_dialog_open = true;
    editor.status = if dirty {
        UNSAVED_PROMPT.into()
    } else {
        "请输入副本名称，副本将归入源场景所在文件夹。".into()
    };
}

pub(crate) fn confirm(
    editor: &mut SceneEditorState,
    task: &mut SceneCreationTask,
    session: &EditorAuthoringSession,
    documents: Option<&SceneDocuments>,
    use_saved_version: bool,
) {
    let Some(target) = editor.new_scene_copy_target.as_ref().filter(|_| editor.new_scene_dialog_open) else {
        return;
    };
    if task.busy() || editor.save_requested || session.saving {
        return; // 确认只产生一张复制票据，重复点击不会接续其他保存。
    }
    let source = target.scene_id.clone();
    let name = match normalize_display_name(&editor.new_scene_name).and_then(|name| {
        scene_directory_name(&name)?;
        Ok(name)
    }) {
        Ok(name) => name,
        Err(error) => {
            editor.status = error;
            return; // 名称未通过同一套新建规则时不触发源场景保存。
        }
    };
    if !use_saved_version && !target.dirty && source_dirty(editor, session, documents, &source) {
        editor.new_scene_copy_target.as_mut().unwrap().dirty = true;
        editor.status = UNSAVED_PROMPT.into();
        return; // 模态打开后收口的作者笔划必须重新给出选择，普通复制按钮不能隐式保存。
    }
    if use_saved_version || !target.dirty {
        start(editor, task, source, name);
        return;
    }
    let mut pending = SceneCopyAfterSave { source, name, stage: SceneCopySaveStage::Activating };
    if source_is_active(editor, session, &pending.source) {
        if !editor.dirty || !session.has_scene_changes() {
            start(editor, task, pending.source, pending.name);
            return; // 场景若已保存，直接用落盘内容，不把仅全局的修改卷入本次操作。
        }
        request_save(editor, session, &mut pending);
    } else if documents.is_some_and(|documents| documents.parked.contains_key(&pending.source)) {
        // 停放文档必须先走原激活链；其他场景的草稿只会停放，不会被保存或丢弃。
        editor.queue_scene_transition(pending.source.clone(), "正在打开源场景，保存成功后继续复制");
    } else {
        editor.status = "源场景作者会话已变化，请重新打开复制表单".into();
        return;
    }
    task.copy_after_save = Some(pending);
}

const UNSAVED_PROMPT: &str = "源场景有未保存修改。可以保存并复制，也可以仅复制已保存版本。";

fn source_dirty(
    editor: &SceneEditorState,
    session: &EditorAuthoringSession,
    documents: Option<&SceneDocuments>,
    source: &SceneId,
) -> bool {
    if source_is_active(editor, session, source) {
        editor.dirty && session.has_scene_changes() && !session.is_unedited_load_migration_for(source)
    } else {
        documents.and_then(|documents| documents.parked.get(source)).is_some_and(|parked| {
            (parked.dirty || parked.session.has_scene_data_changes())
                && !parked.session.is_unedited_load_migration_for(source)
        })
    }
}

fn source_is_active(editor: &SceneEditorState, session: &EditorAuthoringSession, source: &SceneId) -> bool {
    editor.selected_scene_id.as_ref() == Some(source)
        && session.draft.as_ref().is_some_and(|draft| &draft.scene_id == source)
}

fn request_save(editor: &mut SceneEditorState, session: &EditorAuthoringSession, pending: &mut SceneCopyAfterSave) {
    pending.stage = SceneCopySaveStage::Saving {
        revision: session.revision,
        success_before: session.successful_save_sequence,
    };
    editor.save_requested = true;
    editor.status = "正在保存源场景，成功后继续复制".into();
}

/// 保存、场景加载及后台复制各自继续在原任务组运行。等待票据只核对当前源和回执。
pub(crate) fn resume_after_save(
    mut editor: ResMut<SceneEditorState>,
    mut task: ResMut<SceneCreationTask>,
    session: Res<EditorAuthoringSession>,
) {
    if task.copy_after_save.is_none() { return; }
    let Some(mut pending) = task.copy_after_save.take() else { return; };
    if editor.close_requested || !editor.open || !editor.new_scene_dialog_open
        || editor.new_scene_copy_target.as_ref().is_none_or(|target| target.scene_id != pending.source)
    {
        cancel_pending(&mut editor, &pending);
        return;
    }
    match pending.stage {
        SceneCopySaveStage::Activating => {
            if editor.scene_transition_in_progress || editor.pending_scene_transition.as_ref() == Some(&pending.source) {
                task.copy_after_save = Some(pending);
            } else if source_is_active(&editor, &session, &pending.source) {
                if editor.dirty && session.has_scene_changes() {
                    request_save(&mut editor, &session, &mut pending);
                    task.copy_after_save = Some(pending);
                } else {
                    start(&mut editor, &mut task, pending.source, pending.name);
                }
            } else {
                report_failure(&mut editor, &pending, "未能打开源场景");
            }
        }
        SceneCopySaveStage::Saving { revision, success_before } => {
            if editor.save_requested || session.saving {
                task.copy_after_save = Some(pending);
            } else if source_is_active(&editor, &session, &pending.source)
                && session.revision == revision && session.successful_save_sequence != success_before && !editor.dirty
            {
                start(&mut editor, &mut task, pending.source, pending.name);
            } else {
                let diagnostic = editor.status.clone();
                report_failure(&mut editor, &pending, &diagnostic);
            }
        }
    }
}

fn report_failure(editor: &mut SceneEditorState, pending: &SceneCopyAfterSave, problem: &str) {
    editor.status = format!("无法保存并复制场景：{problem}");
    editor.record_operation_result(
        EditorStatusOperation::SceneCopy, &EditorStatusTarget::new(pending.source.as_str(), &pending.name),
        EditorStatusKind::Error,
    );
}

fn start(editor: &mut SceneEditorState, task: &mut SceneCreationTask, source: SceneId, name: String) {
    let control = SceneCopyControl::new();
    let worker = control.clone();
    let asset_root = workspace_asset_root();
    task.ticket = task.ticket.wrapping_add(1);
    let ticket = task.ticket;
    task.feedback_target = Some(EditorStatusTarget::new(source.as_str(), &name));
    task.copy_control = Some(control);
    task.task = Some(IoTaskPool::get().spawn(async move {
        let result = copy_scene(&asset_root, &source, &name, &worker.cancelled, |stage| {
            if let Ok(mut progress) = worker.progress.lock() {
                *progress = stage;
            }
        });
        (ticket, result.map(SceneCreationResult::Copied))
    }));
    editor.status = "正在准备场景复制".into();
}

fn cancel_pending(editor: &mut SceneEditorState, pending: &SceneCopyAfterSave) {
    match pending.stage {
        SceneCopySaveStage::Activating if !editor.scene_transition_in_progress
            && editor.pending_scene_transition.as_ref() == Some(&pending.source) =>
        {
            editor.pending_scene_transition = None;
        }
        SceneCopySaveStage::Saving { .. } if editor.selected_scene_id.as_ref() == Some(&pending.source) => {
            editor.save_requested = false; // 已开始的原子保存仍由原事务正常收口。
        }
        _ => {}
    }
}

/// 返回是否接管这次取消。运行中的复制继续持有模态直到底层事务确认收口。
pub(crate) fn cancel(editor: &mut SceneEditorState, task: &mut SceneCreationTask) -> bool {
    if editor.new_scene_copy_target.is_none() {
        return false;
    }
    if let Some(control) = &task.copy_control {
        control.cancel();
        editor.status = "正在取消场景复制，等待后台清理".into();
        return true;
    }
    if let Some(pending) = task.copy_after_save.take() {
        cancel_pending(editor, &pending);
    }
    editor.new_scene_dialog_open = false;
    editor.new_scene_copy_target = None;
    editor.new_scene_name.clear();
    editor.suppress_world_pick_this_frame = true;
    editor.status = "已取消场景复制".into();
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_copy_invalid_destination_cannot_request_source_save() {
        let source = SceneId::parse("MAP-copy-source").unwrap();
        let mut editor = SceneEditorState {
            open: true, dirty: true, new_scene_dialog_open: true, new_scene_name: "CON".into(),
            new_scene_copy_target: Some(SceneCopyTarget {
                scene_id: source, display_name: "源场景".into(), dirty: true,
            }),
            ..default()
        };
        let mut task = SceneCreationTask::default();
        confirm(&mut editor, &mut task, &EditorAuthoringSession::default(), None, false);
        assert!(editor.status.contains("系统保留名称"));
        assert!(!editor.save_requested && !task.busy());
        assert!(editor.new_scene_dialog_open && editor.dirty);
    }
}
