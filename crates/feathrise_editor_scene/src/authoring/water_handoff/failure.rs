//! 正式水域/摆放任一域失败后的原子作者恢复。

use bevy::prelude::*;

use super::state::WaterFileHandoffPurpose;
use super::{PendingWaterFileHandoff, WaterFileHandoff};
use crate::authoring::resources::EditorAuthoringSession;
use crate::resources::SceneEditorState;

pub(super) fn restore_pending_authoring_inputs(
    pending: &PendingWaterFileHandoff,
    session: &mut EditorAuthoringSession,
) {
    let Some(draft) = session.draft.as_mut() else {
        return;
    };
    if draft.scene_id != pending.gate.scene_id {
        return; // 真正切换草稿后旧场景保存输入不得污染新图。
    }
    super::super::terrain_documents::root_water::restore_dirty(
        draft, pending.dirty_layer_ids.iter().cloned(), pending.catalog_registration_required,
    ); // 即使检查器正聚焦私有画布，也只能恢复根水自己的写入责任。
}

/// 任一正式域确定失败后释放同批全部门、重新标脏并恢复作者关闭入口。
pub(super) fn fail_scene_file_handoff(
    handoff: &mut WaterFileHandoff,
    session: &mut EditorAuthoringSession,
    editor: &mut SceneEditorState,
    reason: &str,
) {
    let mut water = handoff.pending.take(); // 水域与摆放属于同一保存批次，失败时一起释放。
    let placement = handoff.placement.take(); // 是否存在决定下一次原样保存是否需要显式重建。
    let Some(scene_id) = water
        .as_ref()
        .map(|pending| pending.gate.scene_id.clone())
        .or_else(|| {
            placement
                .as_ref()
                .map(|pending| pending.gate.scene_id.clone())
        })
    else {
        return; // 同帧多个失败证据只允许第一个产生一次副作用和诊断。
    };
    if let Some(pending) = water.as_ref()
        && pending.gate.purpose == WaterFileHandoffPurpose::SavePublish
        && (pending.root_pending || !pending.dirty_layer_ids.is_empty() || pending.catalog_registration_required)
    {
        restore_pending_authoring_inputs(pending, session); // 回滚门从未清脏，只有保存发布失败需要恢复作者输入。
    }
    let rollback_restore = water
        .as_ref()
        .is_some_and(|pending| pending.gate.purpose == WaterFileHandoffPurpose::RollbackRestore);
    let placement_retry_required = placement.is_some(); // 已发布但未Ready的摆放下一次只发显式重建请求。
    editor.dirty = true; // 已落盘但未能证明正式运行时采用，必须允许作者重新保存重试。
    editor.cancel_close_request(format!(
        "场景{scene_id}{}失败：{reason}；已进入不可关闭、可重试保存的安全状态",
        if rollback_restore {
            "磁盘回滚版本重载"
        } else if water.is_some() && placement_retry_required {
            "保存后正式水域与摆放交接"
        } else if placement_retry_required {
            "保存后正式摆放交接"
        } else {
            "保存后正式水域交接"
        }
    ));
    handoff.mark_scene_unresolved(scene_id.clone(), reason.to_owned(), placement_retry_required); // 下一次F12不能绕过，同图新保存可以接管修复。
    if let Some(unresolved) = handoff.unresolved.as_mut().filter(|unresolved| unresolved.scene_id == scene_id) {
        if placement.is_some() {
            unresolved.late_placement = placement; // 新摆放门失败才替换旧批；水回滚失败没有新摆放，仍须保留旧严格身份。
        }
        if let Some(mut batch) = water.as_mut().and_then(|pending| pending.terrains.take()) {
            batch.cursor = 0; // 全批脏责任按固定预算恢复；不能只恢复失败时恰好选中的画布。
            unresolved.terrain_recovery = Some(batch);
        } // 没有新私有水批次时保留原恢复游标，不能以None解除旧责任。
    }
    error!("{}", editor.status); // 每次交接最多记录一条明确失败。
}
