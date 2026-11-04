//! 摆放删除意图的文档所有权；迟到正式根不能因作者预览已经销毁而复活。

use std::{collections::HashSet, sync::{Arc, Weak, atomic::{AtomicBool, Ordering}}};

use bevy::prelude::*;
use feathrise_map_content::MapId;
use feathrise_scene::MapPlacementInstance;

use super::EditorAuthoringSession;
use crate::{components::EditorPlacedAsset, ui::placement_water::PlacementInstanceIndex};

mod components;
mod preparation;
mod resources;
mod types;

pub(crate) use components::PlacementDeletionSuppression;
pub(crate) use preparation::apply_prepared;
pub(crate) use resources::PlacementDeletionIndex;
pub(crate) use types::{PlacementDeletionIdentity, PlacementDeletionIntents, PlacementDeletionSnapshot,
    PlacementDeletionSource, PreparedDeletionSnapshot, PreparedPlacementDeletions};
use types::DocumentIndex;

fn document_index(intents: &PlacementDeletionIntents) -> DocumentIndex {
    DocumentIndex { scope: Arc::downgrade(&intents.scope), ids: Arc::downgrade(&intents.ids) }
}

fn register_index(world: &mut World, map: MapId, index: DocumentIndex) {
    // 退休队列可能仍强持旧表，不能凭Weak可升级就回滚活动文档索引。
    // 同scope的排队发布始终取当前表；只有停放文档才使用其冻结的弱索引。
    let index = if let Some(session) = world.get_resource::<EditorAuthoringSession>()
        && session.draft.as_ref().is_some_and(|draft| draft.map_id == map)
        && Arc::downgrade(&session.placement_deletions.scope).ptr_eq(&index.scope) {
        document_index(&session.placement_deletions)
    } else { index };
    if !scope_active(&index.scope) || index.ids.strong_count() == 0 { return; }
    let mut registry = world.get_resource_or_init::<PlacementDeletionIndex>();
    registry.documents.insert(map, index);
    registry.revision = registry.revision.wrapping_add(1);
}

fn scope_active(scope: &Weak<AtomicBool>) -> bool {
    scope.upgrade().is_some_and(|active| active.load(Ordering::Acquire))
}

/// 每次make_mut后必须重登弱表；只在命令执行时发布当前仍存活的完整作者意图。
pub(crate) fn register_document(commands: &mut Commands, map: &MapId, intents: &PlacementDeletionIntents) {
    let (map, index) = (map.clone(), document_index(intents));
    commands.queue(move |world: &mut World| register_index(world, map, index));
}

/// 真正放弃草稿才撤回文档索引，暖停放和冷恢复缓存草稿均不能调用此入口。
pub(crate) fn withdraw_document(commands: &mut Commands, map: &MapId, intents: &PlacementDeletionIntents) {
    intents.scope.store(false, Ordering::Release); // 后台退役仍暂持整表时，也立即撤销旧排队命令的文档信用。
    let (map, scope) = (map.clone(), Arc::downgrade(&intents.scope));
    commands.queue(move |world: &mut World| {
        if let Some(mut index) = world.get_resource_mut::<PlacementDeletionIndex>()
            && index.documents.get(&map).is_some_and(|current| current.scope.ptr_eq(&scope)) {
            index.documents.remove(&map);
            index.revision = index.revision.wrapping_add(1);
        }
    }); // 只撤销一个文档桶；已遮蔽根由现有显隐context的64根预算归还，不排无界世界命令。
}

fn current_deletion(world: &World, map: &MapId, id: &str) -> Option<PlacementDeletionSnapshot> {
    let document = world.get_resource::<PlacementDeletionIndex>()?.documents.get(map)?;
    if !scope_active(&document.scope) { return None; }
    let ids = document.ids.upgrade()?;
    let ticket = Arc::downgrade(ids.get(id)?);
    Some(PlacementDeletionSnapshot { id: id.to_owned(), ticket, scope: document.scope.clone() })
} // 临时strong在返回前释放，绝不与后续作者make_mut并存。

/// 候选表可能强持旧票据，所以排队认领必须核对当前成员，不能只看票据生命。
fn deletion_is_current(world: &World, map: &MapId, frozen: &PlacementDeletionSnapshot) -> bool {
    if !scope_active(&frozen.scope) { return false; }
    if let Some(session) = world.get_resource::<EditorAuthoringSession>()
        && session.draft.as_ref().is_some_and(|draft| &draft.map_id == map)
        && Arc::downgrade(&session.placement_deletions.scope).ptr_eq(&frozen.scope) {
        // 同一系统多次make_mut会使早先registry弱表失效；活动文档仍能证明全部有效成员。
        return session.placement_deletions.ids.get(&frozen.id)
            .is_some_and(|ticket| Arc::downgrade(ticket).ptr_eq(&frozen.ticket));
    }
    current_deletion(world, map, &frozen.id).is_some_and(|current|
        current.scope.ptr_eq(&frozen.scope) && current.ticket.ptr_eq(&frozen.ticket))
}

fn roots_for(world: &World, map: &MapId, id: &str) -> Vec<Entity> {
    world.get_resource::<PlacementInstanceIndex>().map_or_else(Vec::new, |index| index.roots_for(map, id).to_vec())
}

/// 删除责任与摆放使用相同的8192项预算；到限时整笔拒绝，不能遗失仍可能迟到的旧对象。
pub(crate) fn validate_deletions<'a>(session: &EditorAuthoringSession, ids: impl IntoIterator<Item = &'a str>) -> Result<(), String> {
    if session.placement_deletions.write_shared() {
        return Err("摆放删除表正在后台准备，本次删除未执行，请稍后重试".to_owned());
    }
    let mut additional = HashSet::new();
    for id in ids {
        if !session.placement_deletions.ids.contains_key(id) { additional.insert(id); }
        if session.placement_deletions.ids.len() + additional.len() > feathrise_map_content::MAX_MAP_PLACEMENTS_PER_MAP {
            return Err("未确认的摆放删除已达8192项，请先保存并等待正式采用后再删除；本次操作未执行".to_owned());
        }
    }
    Ok(())
}

fn suppress_root(world: &mut World, root: Entity, map: &MapId, deletion: &PlacementDeletionSnapshot) {
    let Some(instance) = world.get::<MapPlacementInstance>(root) else { return; };
    if instance.map_id() != map || instance.id() != deletion.id || !(instance.is_active() || instance.is_pending())
        || world.get::<EditorPlacedAsset>(root).is_some()
        || world.get::<feathrise_scene::StreamedVolumeResident>(root).is_some() { return; }
    if world.get::<Visibility>(root).is_none() { world.entity_mut(root).insert(Visibility::Inherited); }
    let original_visibility = crate::ui::scene_objects::placement_base_visibility(world, root);
    world.entity_mut(root).insert(PlacementDeletionSuppression {
        original_visibility, map: map.clone(), id: deletion.id.clone(), ticket: deletion.ticket.clone(), scope: deletion.scope.clone(), committed: false,
    });
    // 将已删除作者预览的责任转交逻辑删除，避免Undo后残留一个永远不能归还的旧Entity拥有者。
    world.entity_mut(root).remove::<super::placement_preview_ownership::components::PlacementPreviewSuppression>();
    *world.get_mut::<Visibility>(root).expect("上方已补删除根显隐") = Visibility::Hidden;
    feathrise_scene::set_builtin_placement_collision_enabled_world(world, root, false);
}

/// 仅数据事务全部成功后调用；重做删除换票，旧保存不能清掉这次新删除。
pub(crate) fn mark_deleted(commands: &mut Commands, session: &mut EditorAuthoringSession, map: &MapId, id: &str) {
    session.placement_deletions.ids_mut().insert(id.to_owned(), Arc::new(()));
    session.placement_deletions.revision = session.placement_deletions.revision.wrapping_add(1);
    let index = document_index(&session.placement_deletions);
    let deletion = PlacementDeletionSnapshot { id: id.to_owned(),
        ticket: Arc::downgrade(session.placement_deletions.ids.get(id).expect("刚写入删除票据")),
        scope: index.scope.clone() };
    let map = map.clone();
    commands.queue(move |world: &mut World| {
        register_index(world, map.clone(), index);
        if !deletion_is_current(world, &map, &deletion) { return; }
        for root in roots_for(world, &map, &deletion.id) { suppress_root(world, root, &map, &deletion); }
    });
}

/// 恢复记录只撤回当前文档的删除原因；后来的重新删除会使旧排队恢复失去信用。
pub(crate) fn restore_deleted(commands: &mut Commands, session: &mut EditorAuthoringSession, map: &MapId, id: &str) {
    session.placement_deletions.ids_mut().remove(id);
    session.placement_deletions.revision = session.placement_deletions.revision.wrapping_add(1);
    let (map, id, index) = (map.clone(), id.to_owned(), document_index(&session.placement_deletions));
    commands.queue(move |world: &mut World| {
        let scope = index.scope.clone();
        register_index(world, map.clone(), index);
        if !scope_active(&scope) { return; }
        // 原空表可能正在后台退休，Weak仍可升级；不能据此让旧恢复释放后来重新删除的根。
        let active = world.get_resource::<EditorAuthoringSession>().filter(|session|
            session.draft.as_ref().is_some_and(|draft| draft.map_id == map)
                && Arc::downgrade(&session.placement_deletions.scope).ptr_eq(&scope));
        if active.map_or_else(|| current_deletion(world, &map, &id).is_some(),
            |session| session.placement_deletions.ids.contains_key(&id)) { return; }
        for root in roots_for(world, &map, &id) {
            if world.get::<PlacementDeletionSuppression>(root).is_some_and(|mask| mask.scope.ptr_eq(&scope)) {
                release_suppression(world, root);
            }
        }
    });
}

/// 仅消费已成功采用且当前仍有效的视觉队列项；迟到Add直接查整组逻辑表，不等待此队列。
pub(crate) fn reconcile_identity(commands: &mut Commands, map: &MapId, id: &str) {
    let (map, id) = (map.clone(), id.to_owned());
    commands.queue(move |world: &mut World| {
        if let Some(deletion) = current_deletion(world, &map, &id) {
            for root in roots_for(world, &map, &id) { suppress_root(world, root, &map, &deletion); }
        } else {
            let scope = world.get_resource::<PlacementDeletionIndex>()
                .and_then(|index| index.documents.get(&map)).map(|document| document.scope.clone());
            if let Some(scope) = scope {
                for root in roots_for(world, &map, &id) {
                    if world.get::<PlacementDeletionSuppression>(root).is_some_and(|mask| mask.scope.ptr_eq(&scope)) {
                        release_suppression(world, root);
                    }
                }
            }
        }
    });
}

/// 新正式根先检查显式删除，不能把“草稿没有记录”当作删除外部热更新新增对象的依据。
pub(crate) fn reconcile_added_root(world: &mut World, root: Entity) -> bool {
    let Some(instance) = world.get::<MapPlacementInstance>(root) else { return false; };
    let (map, id) = (instance.map_id().clone(), instance.id().to_owned());
    let Some(deletion) = current_deletion(world, &map, &id) else { return false; };
    suppress_root(world, root, &map, &deletion);
    true
}

pub(crate) fn suppression_is_current(world: &World, root: Entity) -> bool {
    let Some(mask) = world.get::<PlacementDeletionSuppression>(root) else { return false; };
    let Some(instance) = world.get::<MapPlacementInstance>(root) else { return false; };
    if instance.map_id() != &mask.map || instance.id() != mask.id { return false; }
    let Some(document) = world.get_resource::<PlacementDeletionIndex>().and_then(|index| index.documents.get(instance.map_id())) else { return false; };
    if !scope_active(&document.scope) || !document.scope.ptr_eq(&mask.scope) { return false; }
    mask.committed || current_deletion(world, instance.map_id(), instance.id())
        .is_some_and(|current| current.ticket.ptr_eq(&mask.ticket))
}

fn release_suppression(world: &mut World, root: Entity) {
    let Some(mask) = world.entity_mut(root).take::<PlacementDeletionSuppression>() else { return; };
    if world.get::<MapPlacementInstance>(root).is_some_and(|instance| instance.map_id() == &mask.map && instance.id() == mask.id && (instance.is_active() || instance.is_pending())) {
        crate::ui::scene_objects::restore_placement_base_visibility(world, root, mask.original_visibility);
        let enabled = world.get::<Visibility>(root) != Some(&Visibility::Hidden);
        feathrise_scene::set_builtin_placement_collision_enabled_world(world, root, enabled);
    }
}

pub(crate) fn release_expired_suppression(world: &mut World, root: Entity) {
    if world.get_entity(root).is_ok() && !suppression_is_current(world, root) { release_suppression(world, root); }
}

/// 严格新Ready只确认冻结的删除版本，旧根保持隐藏到退役；恢复历史可显式撤回该覆盖。
pub(crate) fn acknowledge_saved_deletion(world: &mut World, map: &MapId, saved: &PlacementDeletionSnapshot) -> bool {
    // 保存确认不推进作者session修订，但同样不能在源共享时触发整表写侧复制。
    if world.get_resource::<EditorAuthoringSession>().is_some_and(|session| session.placement_deletions.write_shared()) {
        return false;
    }
    let matching = scope_active(&saved.scope) && world.get_resource::<EditorAuthoringSession>().is_some_and(|session| {
        session.draft.as_ref().is_some_and(|draft| &draft.map_id == map)
            && Arc::downgrade(&session.placement_deletions.scope).ptr_eq(&saved.scope)
            && session.placement_deletions.ids.get(&saved.id).is_some_and(|ticket| Arc::downgrade(ticket).ptr_eq(&saved.ticket))
    });
    if !matching { return true; }
    for root in roots_for(world, map, &saved.id) {
        if let Some(mut mask) = world.get_mut::<PlacementDeletionSuppression>(root)
            && mask.scope.ptr_eq(&saved.scope) && mask.ticket.ptr_eq(&saved.ticket) {
            mask.committed = true;
        }
    }
    let index = {
        let mut session = world.resource_mut::<EditorAuthoringSession>();
        session.placement_deletions.ids_mut().remove(&saved.id);
        session.placement_deletions.revision = session.placement_deletions.revision.wrapping_add(1);
        document_index(&session.placement_deletions)
    };
    register_index(world, map.clone(), index);
    true
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod preparation_tests;
