//! 场景批量保存依次恢复原文档，再消费原save_requested和真实成功序号/文件交接门。

use super::resources::SaveStage;
use crate::{authoring, resources, ui};
use bevy::prelude::*;
use feathrise_scene_content::SceneId;

pub(super) fn begin(world: &mut World, map: Option<&SceneId>) -> Result<SaveStage, String> {
    let editor = world
        .get_resource::<resources::SceneEditorState>()
        .ok_or("场景编辑器尚未初始化")?;
    let session = world
        .get_resource::<authoring::EditorAuthoringSession>()
        .ok_or("场景作者会话尚未初始化")?;
    if let Some(map) = map {
        if editor.selected_scene_id.as_ref() != Some(map)
            || session
                .draft
                .as_ref()
                .is_none_or(|draft| &draft.scene_id != map)
        {
            // 与原场景标签Activate相同的意图入口；不换根、不复制parked草稿、不绕开原交接门。
            world
                .resource_mut::<resources::SceneEditorState>()
                .queue_scene_transition(map.clone(), "保存全部：正在恢复后台场景原草稿与撤销历史");
            return Ok(SaveStage::SelectingScene);
        }
    }
    let global_dirty = world
        .get_resource::<authoring::GlobalEnvironmentAuthoringState>()
        .is_some_and(|state| state.dirty);
    let needs_save = if map.is_some() {
        editor.dirty || session.has_scene_data_changes() || global_dirty
    } else {
        global_dirty
    };
    if !needs_save {
        return Ok(SaveStage::Submitted { scene_sequence: None });
    }
    let before = session.successful_save_sequence;
    world
        .resource_mut::<resources::SceneEditorState>()
        .save_requested = true;
    Ok(SaveStage::Submitted {
        scene_sequence: Some(before),
    })
}

pub(super) fn selected(world: &World, map: &SceneId) -> Result<bool, String> {
    let editor = world
        .get_resource::<resources::SceneEditorState>()
        .ok_or("场景编辑器会话已变化")?;
    if editor.pending_scene_transition.is_some() || editor.scene_transition_in_progress {
        return Ok(false);
    }
    if let Some(documents) = world.get_resource::<ui::scene_documents::SceneDocuments>()
        && (documents.history_task.is_some()
            || documents.pending_history.is_some()
            || documents.preparing_target.is_some())
    {
        return Ok(false);
    }
    if editor.selected_scene_id.as_ref() == Some(map)
        && world
            .get_resource::<authoring::EditorAuthoringSession>()
            .and_then(|session| session.draft.as_ref())
            .is_some_and(|draft| &draft.scene_id == map)
    {
        return Ok(true);
    }
    Err(format!(
        "无法恢复场景{map}，当前草稿保留：{}",
        editor.status
    ))
}

pub(super) fn poll(
    world: &World,
    map: Option<&SceneId>,
    before: Option<u64>,
) -> Result<bool, String> {
    let editor = world
        .get_resource::<resources::SceneEditorState>()
        .ok_or("场景编辑器会话已变化")?;
    let session = world
        .get_resource::<authoring::EditorAuthoringSession>()
        .ok_or("作者会话已变化")?;
    if editor.save_requested || session.saving {
        return Ok(false);
    }
    if let Some(map) = map {
        if !selected(world, map)? {
            return Ok(false);
        }
    }
    if before.is_some_and(|before| session.successful_save_sequence == before) {
        return Err(format!(
            "原场景保存没有成功回执，修改保留：{}",
            editor.status
        ));
    }
    if editor.dirty
        || session.has_scene_data_changes()
        || world
            .get_resource::<authoring::GlobalEnvironmentAuthoringState>()
            .is_some_and(|state| state.dirty)
    {
        return Err(format!("原场景保存未完成全部修改：{}", editor.status));
    }
    if let Some(draft) = session.draft.as_ref() {
        if let Some(handoff) = world.get_resource::<authoring::WaterFileHandoff>() {
            if let Some(error) = handoff.unresolved_reason_for(&draft.scene_id) {
                return Err(format!(
                    "场景正式水域/摆放采用失败：{error}；草稿保留，可重新保存"
                ));
            }
            if handoff.pending_for(
                &draft.scene_id,
                session.water_revision,
                session.terrain_revision,
            ) {
                return Ok(false);
            }
        }
    }
    Ok(true)
}
