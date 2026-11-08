//! 整网格水保存证据根的生命周期登记与相关Scene0资产事件筛选。

use bevy::asset::{AssetEvent, AssetId}; // Scene0热更消息只以类型化ID命中冻结候选。
use bevy::ecs::message::{MessageCursor, Messages}; // 手工游标必须显式检测双缓冲淘汰的未读消息。
use bevy::prelude::*; // Add、Replace、Remove observer与资源访问。
use bevy::world_serialization::WorldAsset; // 保存证据只关注GLB/GLTF Scene0资产。
use feathrise_scene::{DynamicWaterMeshOverrideStatus, MapPlacementWaterEvidence}; // marker与运行时回执共同驱动增量缓存。

use crate::authoring::resources::{
    PlacementWaterEvidenceRegistry, PlacementWaterSavePreflightCache, PlacementWaterStatusEvent,
}; // 全局登记表与单次保存缓存分离所有权。

/// 活动、候选、作者预览及热重载旧根并存时允许登记的工程硬上限。
pub(super) const MAX_PLACEMENT_WATER_EVIDENCE_ROOTS: usize = 4_096;
/// 状态事件环只需覆盖证据根工程上限；落后更多时保存缓存转为全候选分帧复验。
const MAX_PLACEMENT_WATER_STATUS_EVENTS: usize = MAX_PLACEMENT_WATER_EVIDENCE_ROOTS;
/// 单帧最多消费的WorldAsset消息数；其他独立读者不受本游标影响。
pub(super) const PLACEMENT_WATER_ASSET_EVENT_BUDGET_PER_FRAME: usize = 64;

/// O(1)把新证据根追加到连续登记表，异常爆量明确失败关闭而不截断证明。
pub(crate) fn register_placement_water_evidence_root(
    add: On<Add<MapPlacementWaterEvidence>>, // marker落地当下取得精确实体代次。
    statuses: Query<(), With<DynamicWaterMeshOverrideStatus>>, // 同一Bundle若已带状态则补一份当前根专属序号基线。
    mut registry: ResMut<PlacementWaterEvidenceRegistry>, // 唯一生命周期写侧维护连续表和反向索引。
) {
    if registry.indices.contains_key(&add.entity) {
        return; // 防御重复Add通知，不能重复增加全局根计数。
    }
    registry.total_count = registry.total_count.saturating_add(1); // 极端溢出保持最大值并由overflow失败关闭。
    registry.membership_revision = registry.membership_revision.wrapping_add(1); // 任一新增都会作废旧实体游标。
    if registry.entities.try_reserve(1).is_err() || registry.indices.try_reserve(1).is_err() || registry.overflowed {
        registry.overflowed = true; // 已经遗漏过根后保持失败关闭，直到全部异常根真正释放。
        return;
    }
    let index = registry.entities.len(); // 连续下标供保存请求按固定切片直接读取。
    registry.entities.push(add.entity); // 只追加轻量Entity，不复制组件或场景数据。
    registry.indices.insert(add.entity, index); // Remove observer之后可以O(1)交换删除。
    if statuses.get(add.entity).is_ok() {
        record_placement_water_status_event(&mut registry, add.entity); // observer注册顺序不能让同Bundle初始状态缺少实体级授信序号。
    }
}

/// O(1)移除marker已删除或实体已despawn的证据根。
pub(crate) fn unregister_placement_water_evidence_root(
    remove: On<Remove<MapPlacementWaterEvidence>>, // Remove在实体组件不可再查询时仍保留原Entity身份。
    mut registry: ResMut<PlacementWaterEvidenceRegistry>, // 成员代次确保在途保存不会把失效根继续算Ready。
) {
    registry.total_count = registry.total_count.saturating_sub(1); // 每个真实Remove与一次Add配对。
    registry.membership_revision = registry.membership_revision.wrapping_add(1); // 删除立即撤销所有旧聚合信用。
    if let Some(index) = registry.indices.remove(&remove.entity) {
        let removed = registry.entities.swap_remove(index); // 固定O(1)回收，不搬移完整后缀。
        debug_assert_eq!(removed, remove.entity); // 连续表与反向索引必须始终指向同一实体。
        if index < registry.entities.len() {
            let moved = registry.entities[index]; // 原末项补入删除槽后需要修正反向下标。
            registry.indices.insert(moved, index); // 后续移除仍保持O(1)。
        }
    }
    registry.status_revisions_by_entity.remove(&remove.entity); // 已退役实体的旧状态序号不能授信复用下标的新根。
    if registry.total_count == 0 {
        registry.entities.clear(); // 全部根释放后连续表必为空，保留容量供下一地图复用。
        registry.indices.clear(); // 防御异常爆量期间遗漏的反向项残留。
        registry.overflowed = false; // 只有完全清空才能重新取得一份无遗漏登记表。
    }
}

/// 把一次状态生命周期变化追加到固定容量序号环。
fn record_placement_water_status_event(
    registry: &mut PlacementWaterEvidenceRegistry, // observer同步写入，不经过每帧Changed全量Query。
    entity: Entity,                                // 保存缓存之后只直接查询这一实体。
) {
    registry.status_revision = registry.status_revision.saturating_add(1); // 单调序号饱和也不回绕成旧信用。
    let revision = registry.status_revision; // 推入事件前复制轻量序号，避免同一可变借用内再读取资源。
    registry.status_events.push_back(PlacementWaterStatusEvent {
        revision, // 事件与资源发布使用同一序号。
        entity,   // Entity代次避免复用旧下标。
    });
    if registry.indices.contains_key(&entity) {
        registry.status_revisions_by_entity.insert(entity, revision); // 只索引最多4096个marker根，普通placement状态不会扩大缓存。
    }
    if registry.status_events.len() > MAX_PLACEMENT_WATER_STATUS_EVENTS {
        registry.status_events.pop_front(); // 慢读者检测序号缺口后改走全候选固定预算复验。
    }
}

/// 首次安装Pending/Ready/Failed时唤醒对应保存候选。
pub(crate) fn observe_added_placement_water_status(
    add: On<Add<DynamicWaterMeshOverrideStatus>>, // 首个运行时回执也属于真实状态变化。
    mut registry: ResMut<PlacementWaterEvidenceRegistry>, // 只写固定容量事件环。
) {
    record_placement_water_status_event(&mut registry, add.entity); // 不读取或克隆失败字符串。
}

/// `commands.insert`替换现有状态时唤醒对应保存候选。
pub(crate) fn observe_replaced_placement_water_status(
    replace: On<Discard<DynamicWaterMeshOverrideStatus>>, // Bevy在整组件替换前发布Discard（旧名Replace）。
    mut registry: ResMut<PlacementWaterEvidenceRegistry>, // 稳定帧没有Query扫描。
) {
    record_placement_water_status_event(&mut registry, replace.entity); // 新值落地后保存系统按Entity读取。
}

/// 状态被显式移除或实体销毁时撤销旧Ready/Pending聚合。
pub(crate) fn observe_removed_placement_water_status(
    remove: On<Remove<DynamicWaterMeshOverrideStatus>>, // Remove与marker Remove顺序无关，重复唤醒仍幂等。
    mut registry: ResMut<PlacementWaterEvidenceRegistry>, // 事件环按固定容量吸收同帧生命周期通知。
) {
    record_placement_water_status_event(&mut registry, remove.entity); // marker成员变化另由membership_revision负责。
}

/// 提取Bevy五类WorldAsset消息共享的类型化资产ID。
fn world_asset_event_id(event: &AssetEvent<WorldAsset>) -> AssetId<WorldAsset> {
    match event {
        AssetEvent::Added { id }
        | AssetEvent::Modified { id }
        | AssetEvent::Removed { id }
        | AssetEvent::LoadedWithDependencies { id }
        | AssetEvent::Unused { id } => *id, // 任一生命周期变化都可能改变递归加载或实例化结果。
    }
}

/// 固定预算消费Scene0消息，并只报告当前冻结候选真正引用的资产变化。
pub(crate) fn consume_relevant_world_asset_events(
    events: &Messages<AssetEvent<WorldAsset>>, // 只读标准双缓冲消息集合，不影响其他消费者。
    cursor: &mut MessageCursor<AssetEvent<WorldAsset>>, // Local游标跨帧保留并暴露missed_messages计数。
    cache: &mut PlacementWaterSavePreflightCache, // 完整聚合后已持有资产ID引用计数并锁存失效责任。
    status_revision: u64, // 资产事件到达当下的状态事件尾部，是旧Ready允许使用的闭区间上限。
    save_requested: bool, // 没有保存请求时仍消费历史消息，但不建立无意义复验工作。
) -> Result<bool, usize> {
    let missed = cursor.missed_messages(events); // 必须在read自动前移游标前读取真实丢失量。
    if missed > 0 {
        cursor.clear(events); // 当前保存明确失败后O(1)追到消息尾，下一次人工重试从新事件开始。
        cache.scene_asset_event_backlog_status_floor = None; // 已破损积压代不能继续提供任何状态下限。
        cache.scene_asset_invalidation_pending = true; // 下一次保存必须先按固定预算完整复验候选，不能复用同作者修订旧Ready。
        return Err(missed); // 丢失事件对应哪些Scene0已不可证明，绝不能继续使用缓存Ready启动IO。
    }
    let mut relevant = false; // 同一帧多个相关事件合并成一次全候选有界复验请求。
    let status_floor = cache
        .scene_asset_event_backlog_status_floor
        .unwrap_or(status_revision); // 连续积压的最终批也沿用首批开始值，不能用晚读时已经包含新Ready的尾部。
    for event in cursor
        .read(events)
        .take(PLACEMENT_WATER_ASSET_EVENT_BUDGET_PER_FRAME)
    {
        let asset_id = world_asset_event_id(event); // 同一次消息只提取一次类型化ID。
        if save_requested && cache.scene_asset_counts.contains_key(&asset_id) {
            if matches!(event, AssetEvent::Modified { .. }) {
                cache
                    .scene_asset_status_floors
                    .entry(asset_id)
                    .and_modify(|floor| *floor = (*floor).max(status_floor))
                    .or_insert(status_floor); // 只有Bevy会原地重建既有World实例的Modified需要新状态代际授信。
            }
            relevant = true; // 具体候选仍由后续固定实体预算重新读取AssetServer状态。
        }
    }
    cache.scene_asset_invalidation_pending |= relevant; // 其他保存门提前返回也不能丢失本次GLB失效。
    let caught_up = cursor.is_empty(events); // 未读消息可能包含相关GLB；追平前保存失败关闭等待。
    if caught_up {
        cache.scene_asset_event_backlog_status_floor = None; // 最终批处理完才结束本次连续积压代。
    } else {
        cache
            .scene_asset_event_backlog_status_floor
            .get_or_insert(status_revision); // 首批超预算时冻结起点，后续帧不得抬高到新Ready之后。
    }
    Ok(caught_up) // 下一帧继续从独立MessageCursor游标按固定预算消费。
}
