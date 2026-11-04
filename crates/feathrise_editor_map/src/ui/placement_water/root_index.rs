//! placement实例根的生命周期索引；网格水连续编辑不得按ID扫描整张地图。

use std::collections::HashMap; // 稳定placement ID直接命中同时存在的活动、候选与退役小集合。

use bevy::prelude::*; // Add/Remove observer、Entity、Query与Resource。
use feathrise_map_content::MapId; // 同名placement不能跨地图提供作者写入目标。
use feathrise_scene::{DynamicWaterMeshOverrideStatus, MapPlacementInstance}; // 共享三态门与运行时回执共同维护定向作者状态。

/// 全局实例根按稳定placement ID登记；每个ID只会并存少量批次代次。
///
/// 【功能设计】正式地图可有8192条placement，数值左拖每一档都必须O(1)定位目标。
/// 根的Add/Remove生命周期只更新一个HashMap槽，候选与退役根仍留在同ID小数组中，
/// 最终通过`map_id + is_active + id`三重条件失败关闭。
#[derive(Resource, Debug, Default)]
pub(crate) struct PlacementInstanceIndex {
    roots_by_map: HashMap<MapId, HashMap<String, Vec<Entity>>>, // 同图同ID通常不超过活动、候选和少量退役根；跨图身份独立回收。
    identity_by_root: HashMap<Entity, (MapId, String)>, // Remove观察不依赖已删除组件仍可查询，实体销毁必定撤销索引信用。
    membership_revision: u64, // 根Add/Remove让选中项重新聚合，不依赖Query扫描Changed。
    next_status_revision: u64, // 全局单调票据为每个地图内placement发布状态边沿。
    status_revisions: HashMap<MapId, HashMap<String, u64>>, // 嵌套表允许稳定帧按`&MapId + &str`零分配查询。
}

impl PlacementInstanceIndex {
    pub(crate) fn roots_for(&self, map_id: &MapId, placement_id: &str) -> &[Entity] {
        self.roots_by_map.get(map_id).and_then(|placements| placements.get(placement_id))
            .map_or(&[], Vec::as_slice)
    } // 作者遮蔽同时处理同ID活动/候选代次，不能用仅Active的写入入口遗漏Pending。

    pub(crate) fn membership_revision(&self) -> u64 {
        self.membership_revision // Presentation只比较标量，稳定帧不会枚举根。
    }

    pub(crate) fn status_revision(&self, map_id: &MapId, placement_id: &str) -> u64 {
        self.status_revisions
            .get(map_id)
            .and_then(|placements| placements.get(placement_id))
            .copied()
            .unwrap_or_default() // 当前身份尚无运行时回执时保持零。
    }

    fn record_status_change(&mut self, map_id: &MapId, placement_id: &str) {
        self.next_status_revision = self.next_status_revision.wrapping_add(1).max(1);
        self.status_revisions
            .entry(map_id.clone())
            .or_default()
            .insert(placement_id.to_owned(), self.next_status_revision); // 同ID跨地图拥有独立回执票据，切图不会被另一地图唤醒。
    }

    /// 对当前地图和稳定ID的全部活动根执行小集合回调，不枚举其他placement。
    pub(crate) fn for_each_active_root(
        &self,
        map_id: &MapId,
        placement_id: &str,
        roots: &Query<(Entity, &MapPlacementInstance)>,
        mut visit: impl FnMut(Entity),
    ) {
        let Some(candidates) = self
            .roots_by_map
            .get(map_id)
            .and_then(|placements| placements.get(placement_id))
        else {
            return; // 当前Scene0根尚未生成时保持草稿，运行时状态会继续显示Pending。
        };
        for &entity in candidates {
            if roots.get(entity).is_ok_and(|(_, instance)| {
                instance.map_id() == map_id && instance.id() == placement_id && instance.is_active()
            }) {
                visit(entity); // 正常路径唯一命中；异常重复活动根沿用旧逻辑全部重施。
            }
        }
    }

}

/// 新正式候选、活动批次或作者预览根落地时O(1)登记其稳定ID。
pub(crate) fn register_placement_instance_root(
    add: On<Add, MapPlacementInstance>,
    mut commands: Commands,
    roots: Query<&MapPlacementInstance>,
    mut index: ResMut<PlacementInstanceIndex>,
) {
    let Ok(instance) = roots.get(add.entity) else {
        return; // 防御同帧异常移除；没有完整组件就不建立无身份索引。
    };
    let map_id = instance.map_id().clone();
    let placement_id = instance.id().to_owned();
    let candidates = index
        .roots_by_map
        .entry(map_id.clone())
        .or_default()
        .entry(placement_id.clone())
        .or_default();
    if candidates.contains(&add.entity) {
        return; // 防御重复Add，不推进虚假的成员或状态修订。
    }
    candidates.push(add.entity); // 同一实体只登记一次，候选代次并存由小Vec表达。
    index
        .identity_by_root
        .insert(add.entity, (map_id.clone(), placement_id.clone())); // 反向身份与正向槽在同一次观察中发布。
    index.membership_revision = index.membership_revision.wrapping_add(1); // 新活动候选可能改变当前选择汇总。
    index.record_status_change(&map_id, &placement_id); // 即使状态组件稍后Add，根成员变化也先撤销旧显示信用。
    crate::authoring::placement_preview_ownership::reconcile_added_root(&mut commands, add.entity);
}

/// 根组件移除或实体销毁时从所属ID小集合交换删除。
pub(crate) fn unregister_placement_instance_root(
    remove: On<Remove, MapPlacementInstance>,
    mut index: ResMut<PlacementInstanceIndex>,
) {
    let Some((map_id, placement_id)) = index.identity_by_root.remove(&remove.entity) else {
        return; // 未完成Add登记的异常实体没有可撤销信用。
    };
    let (last_identity_root, remove_root_map) = index
        .roots_by_map
        .get_mut(&map_id)
        .map(|placements| {
            let last_identity_root = placements.get_mut(&placement_id).is_some_and(|candidates| {
                if let Some(position) = candidates
                    .iter()
                    .position(|&entity| entity == remove.entity)
                {
                    candidates.swap_remove(position); // 同图同ID代次小集合无需保持顺序。
                }
                candidates.is_empty()
            });
            if last_identity_root {
                placements.remove(&placement_id); // 最后一根退出后释放本地图中的稳定ID键。
            }
            (last_identity_root, placements.is_empty())
        })
        .unwrap_or((false, false));
    if remove_root_map {
        index.roots_by_map.remove(&map_id); // 不让已经退出的地图留下空根表。
    }
    if last_identity_root {
        let remove_status_map = index
            .status_revisions
            .get_mut(&map_id)
            .is_some_and(|placements| {
                placements.remove(&placement_id); // 最后一根退出后由成员修订负责唤醒，无需永久墓碑。
                placements.is_empty()
            });
        if remove_status_map {
            index.status_revisions.remove(&map_id); // 跨地图同名根仍存活也不能留下旧图墓碑。
        }
    } else {
        index.record_status_change(&map_id, &placement_id); // 同图同ID尚有其他代次时撤销旧聚合状态。
    }
    index.membership_revision = index.membership_revision.wrapping_add(1); // 当前选择即使失去最后一根也会刷新为Pending。
}

/// Pending/Ready/Failed首次插入或替换后，只推进所属地图与placement的状态票据。
pub(crate) fn observe_placement_water_status_inserted(
    insert: On<Insert, DynamicWaterMeshOverrideStatus>,
    roots: Query<&MapPlacementInstance>,
    mut index: ResMut<PlacementInstanceIndex>,
) {
    let identity = roots
        .get(insert.entity)
        .map(|instance| (instance.map_id().clone(), instance.id().to_owned()))
        .ok()
        .or_else(|| index.identity_by_root.get(&insert.entity).cloned());
    if let Some((map_id, placement_id)) = identity {
        index.record_status_change(&map_id, &placement_id); // observer只做小字符串身份和HashMap更新。
    }
}

/// 状态显式移除或根销毁时撤销当前选择的旧Ready/Failed显示信用。
pub(crate) fn observe_placement_water_status_removed(
    remove: On<Remove, DynamicWaterMeshOverrideStatus>,
    mut index: ResMut<PlacementInstanceIndex>,
) {
    let identity = index.identity_by_root.get(&remove.entity).cloned(); // 显式移除状态时根索引仍存在；despawn若已先移除根身份，则成员修订已经完成全部撤销。
    if let Some((map_id, placement_id)) = identity {
        index.record_status_change(&map_id, &placement_id); // 存活根失去状态时立即撤销旧Ready/Failed显示信用。
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use feathrise_map_content::MapPlacementRecord;

    fn record(id: &str) -> MapPlacementRecord {
        MapPlacementRecord {
            mesh_lod: None,
            terrain_binding: None,
            editor: Default::default(),
            terrain_patch: None,
            id: id.to_owned(),
            asset_path: "gltf/water.glb".to_owned(),
            asset_target: Default::default(), // 根索引测试沿用完整Scene0。
            building_preset_id: None,         // 根索引夹具是普通摆放。
            courtyard: None,                  // 独立摆放记录不属于任何院落副本。
            translation: [0.0; 3],
            height_mode: feathrise_map_content::MapPlacementHeightMode::FixedHeight, // 根索引测试不访问R16。
            terrain_offset: 0.0, // 固定高度不携带离地偏移。
            yaw_degrees: 0.0,
            scale: [1.0; 3],
            water_surfaces: Vec::new(),
        }
    }

    #[derive(Resource)]
    struct TestTarget(MapId);

    #[derive(Resource, Default)]
    struct TestMatches(Vec<Entity>);

    fn collect_active_roots(
        index: Res<PlacementInstanceIndex>,
        target: Res<TestTarget>,
        roots: Query<(Entity, &MapPlacementInstance)>,
        mut matches: ResMut<TestMatches>,
    ) {
        matches.0.clear();
        index.for_each_active_root(&target.0, "water", &roots, |entity| matches.0.push(entity));
    }

    #[test]
    fn lifecycle_index_finds_only_matching_active_map_root() {
        let map = MapId::parse("MAP-root-index").expect("测试地图ID合法");
        let other_map = MapId::parse("MAP-root-index-other").expect("另一地图ID合法");
        let mut app = App::new();
        app.init_resource::<PlacementInstanceIndex>()
            .insert_resource(TestTarget(map.clone()))
            .init_resource::<TestMatches>()
            .add_observer(register_placement_instance_root)
            .add_observer(unregister_placement_instance_root)
            .add_observer(observe_placement_water_status_inserted)
            .add_observer(observe_placement_water_status_removed)
            .add_systems(Update, collect_active_roots);
        let expected = app
            .world_mut()
            .spawn(MapPlacementInstance::new(&map, &record("water")))
            .id();
        let other = app
            .world_mut()
            .spawn(MapPlacementInstance::new(&other_map, &record("water")))
            .id();
        app.update();
        assert_eq!(app.world().resource::<TestMatches>().0, vec![expected]); // 跨图同名根不能取得写入资格。
        let first_revision = app
            .world()
            .resource::<PlacementInstanceIndex>()
            .status_revision(&map, "water");
        app.world_mut()
            .entity_mut(expected)
            .insert(DynamicWaterMeshOverrideStatus::Pending);
        let pending_revision = app
            .world()
            .resource::<PlacementInstanceIndex>()
            .status_revision(&map, "water");
        assert!(pending_revision > first_revision); // 首次状态插入只推进所属地图与placement票据。
        app.world_mut()
            .entity_mut(expected)
            .insert(DynamicWaterMeshOverrideStatus::Ready);
        assert!(
            app.world()
                .resource::<PlacementInstanceIndex>()
                .status_revision(&map, "water")
                > pending_revision
        ); // 替换状态通过Insert生命周期事件唤醒，不依赖Changed全表查询。

        app.world_mut().despawn(expected);
        app.update();
        assert!(app.world().resource::<TestMatches>().0.is_empty()); // despawn必须立即撤销旧索引信用。
        assert_eq!(
            app.world()
                .resource::<PlacementInstanceIndex>()
                .status_revision(&map, "water"),
            0,
        ); // 另一地图同名根仍存活时，旧图最后一根退出也必须完整清除状态墓碑。
        assert!(app.world().get_entity(other).is_ok()); // 测试中的另一地图根仍保留。
    }
}
