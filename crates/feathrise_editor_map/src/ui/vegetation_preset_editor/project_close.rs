//! 工程级收尾读取本域全部草稿，显式保存仍调用唯一原目录事务。

use super::types::{PresetDraftMode, VegetationPresetEditorState};
use crate::standalone::project_close::types::{ProjectCloseSnapshot, SaveItem, SaveTarget};
use bevy::prelude::*;
use feathrise_map_content::VegetationPresetRegistry;

pub(crate) fn collect(
    world: &World,
    snapshot: &mut ProjectCloseSnapshot,
    items: &mut Vec<SaveItem>,
) {
    let Some(state) = world.get_resource::<VegetationPresetEditorState>() else {
        return;
    };
    let adoption_failed = state.adoption.is_some()
        && world
            .get_resource::<VegetationPresetRegistry>()
            .is_some_and(|registry| registry.last_error.is_some());
    if state.pending_inspection.is_some()
        || state.inspection_work.is_some()
        || state.write_task.is_some()
        || (state.adoption.is_some() && !adoption_failed)
    {
        snapshot.pending("植被预设任务或正式目录采用尚未完成");
    }
    if state.asset_reference.is_some() {
        snapshot.blocked("请先完成或取消植被模型/LOD资产选择");
    }
    if state.active_field.is_some() {
        snapshot.blocked("请先用Enter提交或Esc取消植被预设字段");
    }
    for tab in &state.tabs {
        if tab.draft.dirty() || matches!(tab.draft.mode, PresetDraftMode::Create) {
            snapshot.draft(
                items,
                SaveTarget::Vegetation(tab.id),
                format!(
                    "植被预设：{} [{}]",
                    tab.draft.descriptor.label, tab.draft.descriptor.id
                ),
            );
        }
    }
}

pub(crate) fn request(world: &mut World, id: u64) -> Result<(), String> {
    world.resource_scope(|world, mut state: Mut<VegetationPresetEditorState>| {
        if state.active_field.is_some() {
            return Err("植被字段尚未提交".into());
        }
        let registry = world
            .get_resource::<VegetationPresetRegistry>()
            .ok_or("植被目录尚未加载")?;
        let tab = state
            .tabs
            .iter()
            .find(|tab| tab.id == id)
            .ok_or("植被草稿已关闭")?;
        let mode = tab.draft.mode.clone();
        let descriptor = tab.draft.descriptor.clone();
        super::persistence::start_upsert(&mut state, registry, &mode, descriptor, id)
    })
}

pub(crate) fn poll(world: &World, id: u64) -> Result<bool, String> {
    let state = world
        .get_resource::<VegetationPresetEditorState>()
        .ok_or("植被作者会话已失效")?;
    if state.busy() {
        if let Some(error) = world
            .get_resource::<VegetationPresetRegistry>()
            .and_then(|registry| registry.last_error.as_deref())
        {
            if state.write_task.is_none() {
                return Err(format!("植被目录采用失败：{error}"));
            }
        }
        return Ok(false);
    }
    let tab = state
        .tabs
        .iter()
        .find(|tab| tab.id == id)
        .ok_or("保存期间植被标签已变化；原草稿没有被自动丢弃")?;
    if tab.draft.dirty() || matches!(tab.draft.mode, PresetDraftMode::Create) {
        return Err(format!("植被保存未完成：{}", state.status));
    }
    Ok(true)
}
