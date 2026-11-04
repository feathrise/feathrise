//! 地表作者的元数据适配；其他应用作者由领域回调提供原事务回执。
use super::{resources::ProjectCloseSaveAdapters, types::*};
use bevy::prelude::*;
use feathrise_editor_surface_model::{SurfacePresetEditorState as Surface, types::SurfaceDraftMode};
pub(super) fn collect(
    world: &World,
    snapshot: &mut ProjectCloseSnapshot,
    items: &mut Vec<SaveItem>,
) {
    super::authors::collect(world, snapshot, items);
    if let Some(state) = world.get_resource::<Surface>() {
        if state.write_task.is_some()
            || (state.adoption.is_some() && state.save_feedback_failure().is_none())
        {
            snapshot.pending("地表预设写入或正式纹理目录采用尚未完成");
        }
        if state.active_field.is_some() {
            snapshot.blocked("请先用Enter提交或Esc取消地表预设字段");
        }
        for tab in &state.tabs {
            if tab.draft.dirty() || matches!(tab.draft.mode, SurfaceDraftMode::Create) {
                snapshot.draft(
                    items,
                    SaveTarget::Surface(tab.id),
                    format!(
                        "地表预设：{} [{}]",
                        tab.draft.descriptor.label, tab.draft.descriptor.id
                    ),
                );
            }
        }
    }
}
pub(super) fn request(world: &mut World, target: &SaveTarget) -> Result<(), String> {
    match *target {
        SaveTarget::Domain(domain, id) => (super::authors::find(world, domain)?.request)(world, id),
        SaveTarget::Surface(id) => {
            let adapter = *world
                .get_resource::<ProjectCloseSaveAdapters>()
                .ok_or("地表作者保存适配未安装，草稿保留")?;
            (adapter.surface)(world, id)
        }
        _ => Err("保存适配器目标类型不匹配".into()),
    }
}
pub(super) fn poll(world: &World, target: &SaveTarget) -> Result<bool, String> {
    match *target {
        SaveTarget::Domain(domain, id) => (super::authors::find(world, domain)?.poll)(world, id),
        SaveTarget::Surface(id) => {
            let state = world.get_resource::<Surface>().ok_or("地表会话已变化")?;
            if let Some((_, error)) = state.save_feedback_failure() {
                return Err(format!("地表正式目录采用失败：{error}"));
            }
            if state.busy() {
                return Ok(false);
            }
            let tab = state
                .tabs
                .iter()
                .find(|tab| tab.id == id)
                .ok_or("地表标签已变化")?;
            if tab.draft.dirty() || matches!(tab.draft.mode, SurfaceDraftMode::Create) {
                return Err(format!("地表保存未完成：{}", state.status));
            }
            Ok(true)
        }
        _ => Err("未知作者保存目标".into()),
    }
}
pub(super) fn catalog_hash(world: &World, target: &SaveTarget) -> Option<String> {
    if let SaveTarget::Domain(domain, id) = *target {
        (super::authors::find(world, domain).ok()?.catalog_hash)(world, id)
    } else {
        None
    }
}
pub(super) fn rebase_own_write(world: &mut World, target: &SaveTarget, before: Option<&str>) {
    if let SaveTarget::Domain(domain, id) = *target {
        if let Ok(adapter) = super::authors::find(world, domain) {
            (adapter.rebase_own_write)(world, id, before);
        }
    }
}
