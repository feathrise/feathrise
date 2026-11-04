//! 工程级收尾读取本域全部草稿，显式保存仍调用唯一原目录事务。

use super::types::{BuildingDraftMode, BuildingPresetEditorState};
use crate::standalone::project_close::types::{ProjectCloseSnapshot, SaveItem, SaveTarget};
use bevy::prelude::*;
use feathrise_map_content::BuildingPresetRegistry;

pub(crate) fn collect(
    world: &World,
    snapshot: &mut ProjectCloseSnapshot,
    items: &mut Vec<SaveItem>,
) {
    let Some(state) = world.get_resource::<BuildingPresetEditorState>() else {
        return;
    };
    let adoption_failed = state.adoption.is_some()
        && world
            .get_resource::<BuildingPresetRegistry>()
            .is_some_and(|registry| registry.last_error.is_some());
    if state.write_task.is_some() || (state.adoption.is_some() && !adoption_failed) {
        snapshot.pending("建筑预设任务或正式目录采用尚未完成");
    }
    if state.active_field.is_some() {
        snapshot.blocked("请先用Enter提交或Esc取消建筑预设字段");
    }
    for tab in &state.tabs {
        if tab.draft.dirty() || matches!(tab.draft.mode, BuildingDraftMode::Create) {
            snapshot.draft(
                items,
                SaveTarget::Building(tab.id),
                format!(
                    "建筑预设：{} [{}]",
                    tab.draft.descriptor.label, tab.draft.descriptor.id
                ),
            );
        }
    }
}

pub(crate) fn request(world: &mut World, id: u64) -> Result<(), String> {
    world.resource_scope(|world, mut state: Mut<BuildingPresetEditorState>| {
        if state.active_field.is_some() {
            return Err("建筑字段尚未提交".into());
        }
        let registry = world
            .get_resource::<BuildingPresetRegistry>()
            .ok_or("建筑目录尚未加载")?;
        let tab = state
            .tabs
            .iter()
            .find(|tab| tab.id == id)
            .ok_or("建筑草稿已关闭")?;
        let mode = tab.draft.mode.clone();
        let descriptor = tab.draft.descriptor.clone();
        super::persistence::start_upsert(&mut state, registry, &mode, descriptor, id)
    })
}

pub(crate) fn poll(world: &World, id: u64) -> Result<bool, String> {
    let state = world
        .get_resource::<BuildingPresetEditorState>()
        .ok_or("建筑作者会话已失效")?;
    if state.busy() {
        if let Some(error) = world
            .get_resource::<BuildingPresetRegistry>()
            .and_then(|registry| registry.last_error.as_deref())
        {
            if state.write_task.is_none() {
                return Err(format!("建筑目录采用失败：{error}"));
            }
        }
        return Ok(false);
    }
    let tab = state
        .tabs
        .iter()
        .find(|tab| tab.id == id)
        .ok_or("保存期间建筑标签已变化；原草稿没有被自动丢弃")?;
    if tab.draft.dirty() || matches!(tab.draft.mode, BuildingDraftMode::Create) {
        return Err(format!("建筑保存未完成：{}", state.status));
    }
    Ok(true)
}
