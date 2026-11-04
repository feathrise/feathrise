//! 静水内部有界块只回收临时差分；按下到松开形成唯一完整历史事务。

use super::super::commands::{WaterStrokeSnapshot}; // 历史快照按完整事务统一裁剪。
use super::super::resources::{EditorAuthoringSession, EditorCommand, WaterBrushStroke}; // 内部块与对外历史共享同一笔划状态。
use crate::resources::MapEditorState; // 收口后发布用户可读状态。

pub(super) fn checkpoint_water_stroke(
    _session: &mut EditorAuthoringSession,
    stroke: &mut WaterBrushStroke,
) {
    stroke.snapshot_changed |= !stroke.mask_before.is_empty() || !stroke.height_before.is_empty(); // 已发生变化跨内部块累计。
    stroke.snapshot_height_changed |= !stroke.height_before.is_empty(); // 只有下挖时历史才携带GLB。
    stroke.mask_before.clear(); // 保留32768临时样本硬门，不积累无限差分。
    stroke.height_before.clear(); // 完整最初值已由Arc快照持有。
}

pub(super) fn finish_water_stroke(
    editor: &mut MapEditorState,
    session: &mut EditorAuthoringSession,
    stroke: &mut WaterBrushStroke,
) {
    if !stroke.active {
        return;
    } // 重复松手不生成新历史。
    checkpoint_water_stroke(session, stroke); // 最后不足预算的块也归入同一事务。
    if stroke.snapshot_changed {
        let snapshot = session.draft.as_ref().and_then(|draft| {
            let layer_id = stroke.layer_id.clone()?; // 稳定水层在整笔期间保持一致。
            let before_mask = stroke.snapshot_mask.clone()?; // 首次写入前已经冻结。
            let after_mask = draft.water_masks.get(&layer_id)?.clone(); // 全部待补段完成后的权威像素。
            let terrain = if stroke.snapshot_height_changed {
                // 复合两侧必须同时齐全。
                Some((
                    stroke.snapshot_terrain.clone()?,
                    draft.terrain_mesh.clone()?,
                )) // 同一历史同时拥有前后河床。
            } else {
                None
            }; // 纯擦除不恢复地形。
            Some(WaterStrokeSnapshot {
                layer_id,
                before_mask,
                after_mask,
                terrain,
            }) // 创建O(1)前后Arc快照。
        });
        if let Some(snapshot) = snapshot {
            session
                .push_bounded_water_history(EditorCommand::WaterStrokeSnapshot(Box::new(snapshot))); // 只在整笔收口时压栈一次。
            editor.status = "静水笔划完成；一次撤销可恢复本次全部水域覆盖".to_owned(); // 普通水笔只改覆盖，历史仍按整笔收口。
        }
    }
    cancel_water_stroke(stroke); // 释放本笔临时引用，不影响命令持有的完整版本。
}

pub(super) fn cancel_water_stroke(stroke: &mut WaterBrushStroke) {
    *stroke = WaterBrushStroke::default(); // 只清理笔划状态；在途CPU句柄属于独立资源，不假装取消已运行任务。
}
