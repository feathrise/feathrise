//! 独立作者预览对同ID正式根的定向遮蔽与归还；正式候选不得拥有保存后的新编辑。

pub(crate) mod components;

use bevy::prelude::*;
use feathrise_scene_content::SceneId;
use feathrise_scene::ScenePlacementInstance;

use self::components::PlacementPreviewSuppression;
use super::EditorAuthoringSession;
use crate::components::EditorPlacedAsset;
use crate::ui::placement_water::PlacementInstanceIndex;

/// Add和替换均在命令实际执行时重验会话，不能捕获尚未落地或已被替换的地址作为拥有权。
pub(crate) fn claim_preview(
    commands: &mut Commands, map: &SceneId, id: &str, owner: Entity,
) {
    let (map, id) = (map.clone(), id.to_owned());
    commands.queue(move |world: &mut World| claim_current_preview(world, &map, &id, owner));
}

fn current_preview(world: &World, map: &SceneId, id: &str) -> Option<Entity> {
    let session = world.get_resource::<EditorAuthoringSession>()?;
    if session.draft.as_ref()?.scene_id != *map { return None; }
    let owner = *session.placement_previews.get(id)?;
    world.get::<EditorPlacedAsset>(owner)?;
    world.get::<Visibility>(owner)?; // 没有可见性契约的陈旧地址不能遮住仍可用的正式模型。
    let instance = world.get::<ScenePlacementInstance>(owner)?;
    if !instance.asset_target().is_builtin() && !instance.is_terrain_patch()
        && world.get::<avian3d::prelude::RigidBody>(owner).is_some() {
        return None; // 带独立刚体的建筑继续走原受检碰撞协议，普通视觉遮蔽不能代替碰撞Ready交接。
    }
    (instance.scene_id() == map && instance.id() == id && instance.is_active()
        && world.get::<feathrise_scene::StreamedVolumeResident>(owner).is_none()).then_some(owner)
}

fn identity_roots(world: &World, map: &SceneId, id: &str) -> Vec<Entity> {
    world.get_resource::<PlacementInstanceIndex>().map_or_else(Vec::new, |index| index.roots_for(map, id).to_vec())
} // 只复制同一稳定ID的少量并存代次，不扫描场景或复制作者水数组。

fn claim_current_preview(world: &mut World, map: &SceneId, id: &str, owner: Entity) {
    if current_preview(world, map, id) != Some(owner) { return; }
    for root in identity_roots(world, map, id) {
        suppress_formal_root(world, root, map, id, owner);
    }
}

fn suppress_formal_root(world: &mut World, root: Entity, map: &SceneId, id: &str, owner: Entity) {
    let Some(instance) = world.get::<ScenePlacementInstance>(root) else { return; };
    if instance.scene_id() != map || instance.id() != id || !(instance.is_active() || instance.is_pending())
        || world.get::<EditorPlacedAsset>(root).is_some()
        || world.get::<feathrise_scene::StreamedVolumeResident>(root).is_some() { return; }
    // Scene0根也允许尚无显式Visibility；补默认值先于遮蔽，显隐observer不会误记自己的Hidden。
    if world.get::<Visibility>(root).is_none() { world.entity_mut(root).insert(Visibility::Inherited); }
    let original_visibility = world.get::<PlacementPreviewSuppression>(root)
        .map(|previous| previous.original_visibility)
        .unwrap_or_else(|| crate::ui::scene_objects::placement_base_visibility(world, root));
    world.entity_mut(root).insert(PlacementPreviewSuppression { owner, original_visibility });
    *world.get_mut::<Visibility>(root).expect("上方已补正式根显隐") = Visibility::Hidden;
    feathrise_scene::set_builtin_placement_collision_enabled_world(world, root, false);
} // 只禁内置根的直接子碰撞；建筑、三维地形及共享批次刚体仍遵守各自正式采用协议。

/// 根生命周期索引先登记，再处理新作者根或迟到的正式候选；激活只改容器，根上的遮蔽持续有效。
pub(crate) fn reconcile_added_root(commands: &mut Commands, root: Entity) {
    commands.queue(move |world: &mut World| {
        if super::placement_deletion::reconcile_added_root(world, root) { return; }
        let Some(instance) = world.get::<ScenePlacementInstance>(root) else { return; };
        let (map, id) = (instance.scene_id().clone(), instance.id().to_owned());
        let Some(owner) = current_preview(world, &map, &id) else { return; };
        if owner == root { claim_current_preview(world, &map, &id, owner); }
        else { suppress_formal_root(world, root, &map, &id, owner); }
    });
}

/// 只有精确保存清理或显式冷载撤回作者覆盖才调用；删除预览不归还，以免复活已删除正式对象。
pub(crate) fn release_preview(world: &mut World, map: &SceneId, id: &str, owner: Entity) {
    for root in identity_roots(world, map, id) {
        let Some(suppression) = world.get::<PlacementPreviewSuppression>(root).copied()
            .filter(|suppression| suppression.owner == owner) else { continue; };
        let Some(instance) = world.get::<ScenePlacementInstance>(root) else { continue; };
        if instance.scene_id() != map || instance.id() != id { continue; }
        let live = instance.is_active() || instance.is_pending();
        world.entity_mut(root).remove::<PlacementPreviewSuppression>();
        if live {
            crate::ui::scene_objects::restore_placement_base_visibility(world, root, suppression.original_visibility);
            let enabled = world.get::<Visibility>(root) != Some(&Visibility::Hidden);
            feathrise_scene::set_builtin_placement_collision_enabled_world(world, root, enabled);
        } // 退役根保持隐藏与原物理门，不能借归还重新获得旧批次信用。
    }
}

pub(crate) fn withdraw_cold_preview(commands: &mut Commands, owner: Entity) {
    commands.queue(move |world: &mut World| {
        let Some(instance) = world.get::<ScenePlacementInstance>(owner) else { return; };
        if world.get::<EditorPlacedAsset>(owner).is_none() { return; }
        let (map, id) = (instance.scene_id().clone(), instance.id().to_owned());
        release_preview(world, &map, &id, owner);
    }); // 暖驻留文档不经过冷清理；不能因暂时移出当前session就撤销其预览遮蔽。
}

#[cfg(test)]
mod tests;
