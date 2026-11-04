//! 地图批量保存依次恢复原文档，再消费原save_requested和真实成功序号/文件交接门。

use super::resources::SaveStage;
use crate::{authoring, resources, ui};
use bevy::prelude::*;
use feathrise_map_content::MapId;

pub(super) fn begin(world: &mut World, map: Option<&MapId>) -> Result<SaveStage, String> {
    let editor = world
        .get_resource::<resources::MapEditorState>()
        .ok_or("地图编辑器尚未初始化")?;
    let session = world
        .get_resource::<authoring::EditorAuthoringSession>()
        .ok_or("地图作者会话尚未初始化")?;
    if let Some(map) = map {
        if editor.selected_map_id.as_ref() != Some(map)
            || session
                .draft
                .as_ref()
                .is_none_or(|draft| &draft.map_id != map)
        {
            // 与原地图标签Activate相同的意图入口；不换根、不复制parked草稿、不绕开原交接门。
            world
                .resource_mut::<resources::MapEditorState>()
                .queue_map_transition(map.clone(), "保存全部：正在恢复后台地图原草稿与撤销历史");
            return Ok(SaveStage::SelectingMap);
        }
    }
    let global_dirty = world
        .get_resource::<authoring::GlobalEnvironmentAuthoringState>()
        .is_some_and(|state| state.dirty);
    let needs_save = if map.is_some() {
        editor.dirty || session.has_map_data_changes() || global_dirty
    } else {
        global_dirty
    };
    if !needs_save {
        return Ok(SaveStage::Submitted { map_sequence: None });
    }
    let before = session.successful_save_sequence;
    world
        .resource_mut::<resources::MapEditorState>()
        .save_requested = true;
    Ok(SaveStage::Submitted {
        map_sequence: Some(before),
    })
}

pub(super) fn selected(world: &World, map: &MapId) -> Result<bool, String> {
    let editor = world
        .get_resource::<resources::MapEditorState>()
        .ok_or("地图编辑器会话已变化")?;
    if editor.pending_map_transition.is_some() || editor.map_transition_in_progress {
        return Ok(false);
    }
    if let Some(documents) = world.get_resource::<ui::map_documents::MapDocuments>()
        && (documents.history_task.is_some()
            || documents.pending_history.is_some()
            || documents.preparing_target.is_some())
    {
        return Ok(false);
    }
    if editor.selected_map_id.as_ref() == Some(map)
        && world
            .get_resource::<authoring::EditorAuthoringSession>()
            .and_then(|session| session.draft.as_ref())
            .is_some_and(|draft| &draft.map_id == map)
    {
        return Ok(true);
    }
    Err(format!(
        "无法恢复地图{map}，当前草稿保留：{}",
        editor.status
    ))
}

pub(super) fn poll(
    world: &World,
    map: Option<&MapId>,
    before: Option<u64>,
) -> Result<bool, String> {
    let editor = world
        .get_resource::<resources::MapEditorState>()
        .ok_or("地图编辑器会话已变化")?;
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
            "原地图保存没有成功回执，修改保留：{}",
            editor.status
        ));
    }
    if editor.dirty
        || session.has_map_data_changes()
        || world
            .get_resource::<authoring::GlobalEnvironmentAuthoringState>()
            .is_some_and(|state| state.dirty)
    {
        return Err(format!("原地图保存未完成全部修改：{}", editor.status));
    }
    if let Some(draft) = session.draft.as_ref() {
        if let Some(handoff) = world.get_resource::<authoring::WaterFileHandoff>() {
            if let Some(error) = handoff.unresolved_reason_for(&draft.map_id) {
                return Err(format!(
                    "地图正式水域/摆放采用失败：{error}；草稿保留，可重新保存"
                ));
            }
            if handoff.pending_for(
                &draft.map_id,
                session.water_revision,
                session.terrain_revision,
            ) {
                return Ok(false);
            }
        }
        if world
            .get_resource::<authoring::TerrainLiveApplyState>()
            .is_some_and(|state| {
                state.file_handoff_pending_for(&draft.map_id, session.terrain_revision)
            })
        {
            return Ok(false);
        }
    }
    Ok(true)
}
