//! 正式摆放严格新 Ready 后的精确作者预览回收。

use bevy::prelude::*;
use feathrise_scene::{MapPlacementRuntimePhase, MapPlacementRuntimeStatus};

use super::{PendingPlacementFileHandoff, PlacementDeletionSnapshot, PlacementPreviewCleanup, WaterFileHandoff};
use crate::authoring::resources::{EditorAuthoringSession, PlacementPreviewHandoffSnapshot};

const PLACEMENT_PREVIEW_CLEANUP_BUDGET_PER_FRAME: usize = 64; // 直接根静默、层级断边和叶销毁共用固定64个世界操作额度。

/// 与正式摆放完成门相同的严格身份；离图放行绝不能销毁另一草稿的预览。
pub(super) fn placement_strictly_ready(
    pending: &PendingPlacementFileHandoff,
    status: &MapPlacementRuntimeStatus,
) -> bool {
    status.map_id.as_ref() == Some(&pending.gate.map_id)
        && status.phase == MapPlacementRuntimePhase::Ready
        && status.revision > pending.gate.baseline_revision
        && status.attempt_revision > pending.gate.baseline_attempt_revision
}

/// 把一个已经由严格Ready证明可替代的精确保存批次移交给独立清理系统。
pub(crate) fn schedule_placement_preview_cleanup(
    handoff: &mut WaterFileHandoff,
    map_id: &feathrise_map_content::MapId,
    previews: Vec<PlacementPreviewHandoffSnapshot>,
) -> Result<(), Vec<PlacementPreviewHandoffSnapshot>> {
    schedule_placement_save_cleanup(handoff, map_id, previews, Vec::new())
        .map_err(|(previews, _)| previews) // 无门路径只回收预览，不能冒用它确认逻辑删除。
}

/// 严格Ready证明本批正式采用后，预览回收与精确删除确认共享一个有界槽。
pub(super) fn schedule_placement_save_cleanup(
    handoff: &mut WaterFileHandoff,
    map_id: &feathrise_map_content::MapId,
    previews: Vec<PlacementPreviewHandoffSnapshot>,
    deleted: Vec<PlacementDeletionSnapshot>,
) -> Result<(), (Vec<PlacementPreviewHandoffSnapshot>, Vec<PlacementDeletionSnapshot>)> {
    if previews.is_empty() && deleted.is_empty() {
        return Ok(());
    }
    if handoff.preview_cleanup.is_none() {
        handoff.preview_cleanup = Some(PlacementPreviewCleanup {
            map_id: map_id.clone(), // Ready后清理阶段继续为同图作者写入提供精确冻结身份。
            previews,               // 同一资源只允许一个批次，最大仍是保存前8192项硬预算。
            deleted,                // 弱票据不会把被撤销的删除延寿，也不会清掉后来同ID的新删除。
            deletion_cursor: 0,
            quiesce_cursor: 0,      // 第一阶段尚未静默任何直接预览根。
            tree_cursor: 0,         // 第二阶段尚未开始回收树。
            stack: Vec::new(),      // DFS栈按当前树实际深度渐进增长。
        });
        Ok(())
    } else {
        Err((previews, deleted)) // 调用方必须保留原拥有权并下帧重试，绝不丢失或无界合并后续批次。
    }
}

/// Failed/超时后候选若迟到完成，也只用原门的严格修订身份清理原实体快照。
pub(super) fn schedule_late_ready_placement_preview_cleanup(
    handoff: &mut WaterFileHandoff,
    status: &MapPlacementRuntimeStatus,
    active_map_id: Option<&feathrise_map_content::MapId>,
) {
    let ready = handoff
        .unresolved
        .as_ref()
        .and_then(|unresolved| unresolved.late_placement.as_ref())
        .is_some_and(|pending| placement_strictly_ready(pending, status));
    if !ready {
        return; // 未解决稳定帧只比较公开状态标量，不遍历或查询任何世界实体。
    }
    if handoff.preview_cleanup.is_some() {
        return; // 前一精确批次分帧完成后再取得late_placement，原Vec所有权仍保留在未解决门。
    }
    let pending = handoff
        .unresolved
        .as_mut()
        .and_then(|unresolved| unresolved.late_placement.take())
        .expect("上方严格Ready已确认迟到摆放门存在");
    let map_id = pending.gate.map_id.clone();
    let deleted = if active_map_id == Some(&map_id) { pending.deleted } else { Vec::new() };
    schedule_placement_save_cleanup(handoff, &map_id, pending.editor_previews, deleted)
        .expect("上方已确认清理槽为空");
}

/// 严格Ready边沿按固定世界操作预算静默并非递归回收精确预览树。
pub(crate) fn cleanup_placement_handoff_previews(world: &mut World) {
    if world
        .resource::<WaterFileHandoff>()
        .preview_cleanup
        .is_none()
    {
        return; // 空闲稳定帧只读短路，不能把交接资源伪标Changed并唤醒属性同步。
    }
    let Some(mut cleanup) = world
        .resource_mut::<WaterFileHandoff>()
        .preview_cleanup
        .take()
    else {
        unreachable!("上方独占World只读检查后清理状态不会并发消失");
    };
    let mut budget = PLACEMENT_PREVIEW_CLEANUP_BUDGET_PER_FRAME;

    // 删除票据只在严格Ready后到达本槽；逐项比较身份，不能按保存成功清空当前文档。
    while budget > 0 && cleanup.deletion_cursor < cleanup.deleted.len() {
        let deleted = &cleanup.deleted[cleanup.deletion_cursor];
        if !crate::authoring::placement_deletion::acknowledge_saved_deletion(world, &cleanup.map_id, deleted) {
            // 后台尚在读取当前表时保持同一确认项，不能触发COW整表复制或先清掉它对应的预览。
            budget = 0;
            break;
        }
        cleanup.deletion_cursor += 1;
        budget -= 1;
    }

    // 第一阶段先停止全部直接Scene0根热更；每根隐藏、注销和索引迁移合并为一个固定操作。
    while budget > 0 && cleanup.quiesce_cursor < cleanup.previews.len() {
        let preview = &mut cleanup.previews[cleanup.quiesce_cursor];
        cleanup.quiesce_cursor += 1;
        budget -= 1;
        if !preview.matches_world(world, &cleanup.map_id) {
            continue; // 旧Ready不能静默、注销或遍历同Entity上的新作者版本，索引也继续归后来版本所有。
        }
        preview.cleanup_owned = true;
        let (id, root) = (&preview.id, preview.root);
        let still_current = world.resource::<EditorAuthoringSession>().placement_previews.get(id) == Some(&root);
        if world.get::<feathrise_scene::StreamedVolumeResident>(root).is_none() {
            feathrise_scene::set_builtin_placement_collision_enabled_world(world, root, false);
            feathrise_scene::set_builtin_placement_batch_enabled_world(world, root, false);
        } // 粘贴预览可能共享父刚体；移除MPI之前同步禁用唯一子碰撞，不能只禁一个没有刚体的根。
        if let Ok(mut root_mut) = world.get_entity_mut(root) {
            if !root_mut.contains::<feathrise_scene::StreamedVolumeResident>() {
                // DFS可跨帧；归还正式根之前先停独立作者刚体和角色承托，避免同帧双碰撞。
                root_mut.insert(Visibility::Hidden);
                root_mut.remove::<(WorldAssetRoot, feathrise_scene::MapPlacementInstance)>(); // 同时停止热更并撤销作者根索引资格，跨帧DFS期间不能被输入再次接管。
            } // 已被拓扑域拥有的驻留根继续承托；保存只归还作者索引，不销毁活动窗口。
        }
        if still_current {
            world
                .resource_mut::<EditorAuthoringSession>()
                .placement_previews
                .remove(id); // 只迁移仍映射本批原实体的索引，后续同ID预览保持可达。
            crate::authoring::placement_preview_ownership::release_preview(world, &cleanup.map_id, id, root);
        }
    }

    // 第二阶段逐边断开linked hierarchy，再只销毁已经没有Children的叶实体。
    while budget > 0 && cleanup.quiesce_cursor == cleanup.previews.len() {
        if cleanup.stack.is_empty() {
            if cleanup.tree_cursor >= cleanup.previews.len() {
                break; // 全部直接根及其后代已经完整回收。
            }
            let preview = &cleanup.previews[cleanup.tree_cursor];
            if !preview.cleanup_owned {
                cleanup.tree_cursor += 1;
                budget -= 1;
                continue; // 第一阶段身份失配的整条记录不进入DFS，也不改变后来预览的任何组件。
            }
            let root = preview.root;
            if world
                .get::<feathrise_scene::StreamedVolumeResident>(root)
                .is_some()
            {
                cleanup.tree_cursor += 1;
                budget -= 1;
                continue;
            }
            if world.get_entity(root).is_err() {
                cleanup.tree_cursor += 1;
                budget -= 1; // 外部提前销毁同样消费一个幂等检查额度。
                continue;
            }
            cleanup.stack.push(root);
        }
        let entity = *cleanup.stack.last().expect("上方已建立当前树DFS根");
        if world.get_entity(entity).is_err() {
            cleanup.stack.pop();
            budget -= 1;
        } else if world.get::<WorldAssetRoot>(entity).is_some() {
            let mut entity_mut = world.entity_mut(entity);
            entity_mut.insert(Visibility::Hidden);
            entity_mut.remove::<WorldAssetRoot>(); // 嵌套World根也必须先停止热更。
            budget -= 1;
        } else if let Some(child) = world
            .get::<Children>(entity)
            .and_then(|children| children.last().copied())
        {
            if let Ok(mut child_mut) = world.get_entity_mut(child) {
                child_mut.insert(Visibility::Hidden);
                child_mut.remove::<ChildOf>(); // 先断linked关系，父节点最终despawn不会递归宽树。
                cleanup.stack.push(child);
            }
            budget -= 1;
        } else {
            let _ = world.despawn(entity); // 只销毁已经确认无Children的一个叶节点。
            cleanup.stack.pop();
            budget -= 1;
        }
        if cleanup.stack.is_empty() {
            cleanup.tree_cursor += 1; // 当前直接根完整消失后才推进下一棵树。
        }
    }

    let finished = cleanup.deletion_cursor == cleanup.deleted.len()
        && cleanup.quiesce_cursor == cleanup.previews.len()
        && cleanup.tree_cursor == cleanup.previews.len()
        && cleanup.stack.is_empty();
    if !finished {
        world.resource_mut::<WaterFileHandoff>().preview_cleanup = Some(cleanup);
    } else {
        // 世界清理已完成，但完整ID、弱票和预览数组仍按规模析构；O(1)移交共享后台回收器。
        world.get_resource_or_init::<crate::authoring::AuthoringRetirement>().push(cleanup);
    } // 完整收口帧保持None，随后保存/关闭可继续，下一Input帧网格水正常解冻。
}

#[cfg(test)]
mod tests;
