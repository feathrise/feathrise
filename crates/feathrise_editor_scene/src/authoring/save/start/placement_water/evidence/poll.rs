//! 已冻结整网格水候选的状态事件、runtime/GLB换代与稳定Pending定向轮询。

use std::collections::BTreeMap; // 当前session预览所有权以稳定placement ID精确认领。
use std::ops::Bound::{Excluded, Unbounded}; // Pending候选公平轮询从上次下标之后继续。

use bevy::asset::{DependencyLoadState, LoadState, RecursiveDependencyLoadState}; // 递归Scene0依赖确定失败必须停止等待。
use bevy::prelude::*; // AssetServer、Entity与Query直接实体访问。
use bevy::world_serialization::{WorldAsset, WorldAssetRoot}; // 根强句柄只读加载状态，不发起新IO。
use feathrise_scene_content::{SceneId, ScenePlacementRecord}; // 保存请求已经冻结的非空水记录。
use feathrise_scene::{
    DynamicWaterMeshOverrideStatus, DynamicWaterMeshOverrides, ScenePlacementInstance,
    ScenePlacementWaterEvidence,
}; // 精确根快照、三态gate和运行时回执共同组成保存证明。

use super::{gate_candidate_observation, request_forced_candidate_poll, update_candidate}; // 聚合写侧保持在父模块，轮询只提供新观察。
use crate::authoring::resources::{
    PlacementWaterCandidateObservation, PlacementWaterEvidenceRegistry,
    PlacementWaterSavePreflightCache,
}; // 单次保存缓存保存全部有界游标与聚合计数。
use crate::authoring::save::start::placement_water::placement_water_root_scope; // 复用严格根所有权分类。
use crate::components::EditorPlacedAsset; // 精确区分当前session预览与正式/候选根。

/// 单帧最多重新查询的已冻结候选实体数量。
pub(super) const PLACEMENT_WATER_CANDIDATE_POLL_BUDGET_PER_FRAME: usize = 32;
/// 单帧最多推进的状态事件序号；非候选事件只做O(1)跳过。
const PLACEMENT_WATER_STATUS_EVENT_BUDGET_PER_FRAME: usize = 64;

/// 只读当前预览/正式根已有强Scene0句柄的AssetServer状态，不发起加载或IO。
fn placement_scene_load_failed(asset_server: &AssetServer, root: &WorldAssetRoot) -> bool {
    asset_server
        .get_load_states(root.0.id())
        .is_some_and(|(direct, dependencies, recursive)| {
            matches!(direct, LoadState::Failed(_))
                || matches!(dependencies, DependencyLoadState::Failed(_))
                || matches!(recursive, RecursiveDependencyLoadState::Failed(_))
        }) // GLB本体、材质、纹理或Mesh任一递归依赖失败都不会自行进入World ready。
}

/// 当前根相对冻结记录的完整观察；None只表示不能作为这条记录的候选。
#[allow(clippy::type_complexity)] // 查询元组必须在一次直接实体读取中冻结完整证明字段。
pub(super) fn inspect_candidate_root(
    entity: Entity,              // 由登记表或候选槽提供的精确实体代次。
    scene_id: &SceneId,              // 保存请求冻结场景身份。
    record: &ScenePlacementRecord, // 保存请求冻结完整placement和水数组。
    roots: &Query<
        (
            &ScenePlacementInstance,
            &DynamicWaterMeshOverrides,
            Option<&DynamicWaterMeshOverrideStatus>,
            &WorldAssetRoot,
            Option<&EditorPlacedAsset>,
        ),
        With<ScenePlacementWaterEvidence>,
    >, // 只用Query::get，不从首项枚举整个世界。
    asset_server: &AssetServer,  // 只读当前句柄直接与递归加载失败。
    session_previews: &BTreeMap<String, Entity>, // 作者预览还必须由当前session精确实体表拥有。
) -> Option<(
    PlacementWaterCandidateObservation,
    bevy::asset::AssetId<WorldAsset>,
)> {
    let Ok((instance, overrides, status, root, editor_preview)) = roots.get(entity) else {
        return None; // marker已移除或实体已despawn，旧Ready立即失去资格。
    };
    placement_water_root_scope(entity, instance, editor_preview.is_some(), session_previews)?; // 退役正式根和非本session预览均失败关闭为无候选。
    if !instance.matches_record(scene_id, record, overrides.surfaces()) {
        return None; // 跨场景、错资产、错变换或旧水数组不能提供当前快照证明。
    }
    let observation = match status {
        Some(status) if status.is_ready() => PlacementWaterCandidateObservation::Ready, // Ready优先于旧加载失败缓存。
        Some(status) if status.failure_detail().is_some() => {
            PlacementWaterCandidateObservation::Failed(
                status
                    .failure_detail()
                    .expect("分支已经确认失败详情存在")
                    .to_owned(),
            ) // 只在真实状态变化或首次聚合时克隆具体作者诊断。
        }
        Some(_) | None if placement_scene_load_failed(asset_server, root) => {
            PlacementWaterCandidateObservation::Failed(format!(
                "Scene0或其依赖加载失败：{}",
                record.asset_path,
            )) // World ready不会到达时停止无限Pending。
        }
        Some(_) | None => PlacementWaterCandidateObservation::Pending, // 当前句柄仍可能合法收敛。
    };
    Some((observation, root.0.id())) // 资产ID随同一次根查询冻结，热更只唤醒相关候选。
}

/// 直接复验一个已经冻结的候选实体。
#[allow(clippy::too_many_arguments)] // 完整身份字段必须在同一次实体读取中保持一致。
fn poll_candidate(
    cache: &mut PlacementWaterSavePreflightCache, // 候选槽和聚合计数唯一写侧。
    registry: &PlacementWaterEvidenceRegistry,    // 当前实体最近状态序号为AssetEvent代际门授信。
    candidate_index: usize,                       // 状态事件或固定轮询提供的连续槽。
    scene_id: &SceneId,                               // 当前保存请求冻结场景。
    roots: &Query<
        (
            &ScenePlacementInstance,
            &DynamicWaterMeshOverrides,
            Option<&DynamicWaterMeshOverrideStatus>,
            &WorldAssetRoot,
            Option<&EditorPlacedAsset>,
        ),
        With<ScenePlacementWaterEvidence>,
    >, // Query::get保持每次O(1)实体定位。
    asset_server: &AssetServer,                   // 只读当前Scene0加载失败。
    session_previews: &BTreeMap<String, Entity>,  // 预览所有权可能随保存/切换场景撤销。
) {
    let entity = cache.candidates[candidate_index].entity; // 先复制轻量Entity释放cache借用。
    let record_index = cache.candidates[candidate_index].record_index; // 冻结记录槽不会在证据阶段变化。
    let inspection = inspect_candidate_root(
        entity,
        scene_id,
        &cache.water_records[record_index],
        roots,
        asset_server,
        session_previews,
    ); // 本次完整读取结束后再可变更新聚合。
    let (observation, asset_id) = inspection
        .map(|(observation, asset_id)| (observation, Some(asset_id)))
        .unwrap_or((PlacementWaterCandidateObservation::Ineligible, None)); // marker删除、despawn、退役或快照漂移全部撤销旧信用。
    let status_revision = registry.status_revisions_by_entity.get(&entity).copied(); // 事件环溢出后仍能按实体取得最新明确状态信用。
    let observation = gate_candidate_observation(
        cache,
        candidate_index,
        asset_id,
        status_revision,
        observation,
    ); // 相关AssetEvent之后重新读到旧Ready只能继续解释为Pending。
    update_candidate(cache, candidate_index, observation, asset_id); // 只重派生所属记录和全局首问题。
}

/// 消费固定数量状态事件，只复验事件对应的当前候选实体。
#[allow(clippy::too_many_arguments)] // 与poll_candidate共享同一完整身份参数集。
pub(super) fn poll_changed_candidates(
    cache: &mut PlacementWaterSavePreflightCache, // 保存缓存持有事件游标与候选反向索引。
    registry: &PlacementWaterEvidenceRegistry,    // 固定容量状态事件环。
    scene_id: &SceneId,                               // 保存请求冻结场景。
    roots: &Query<
        (
            &ScenePlacementInstance,
            &DynamicWaterMeshOverrides,
            Option<&DynamicWaterMeshOverrideStatus>,
            &WorldAssetRoot,
            Option<&EditorPlacedAsset>,
        ),
        With<ScenePlacementWaterEvidence>,
    >, // 状态事件只按Entity直接读取。
    asset_server: &AssetServer,                   // 当前Scene0加载结果。
    session_previews: &BTreeMap<String, Entity>,  // 当前session精确预览归属。
    poll_budget: &mut usize,                      // 与强制/Pending轮询共享单帧候选查询硬预算。
) -> usize {
    let mut event_budget = PLACEMENT_WATER_STATUS_EVENT_BUDGET_PER_FRAME; // 非候选事件同样不能单帧无界消费。
    let mut polled = 0; // 返回真实Query::get候选次数供测试锁定。
    while cache.status_event_cursor < registry.status_revision && event_budget > 0 {
        let Some(oldest) = registry.status_events.front().map(|event| event.revision) else {
            request_forced_candidate_poll(cache); // 序号前进但事件环为空时保守复验全部候选。
            cache.status_event_cursor = registry.status_revision; // 当前完整周期接管丢失通知责任。
            break;
        };
        let next = cache.status_event_cursor.saturating_add(1); // 状态序号不允许回绕成旧信用。
        if next < oldest {
            request_forced_candidate_poll(cache); // 保存读者落后于事件环，不能猜测漏掉哪些实体。
            cache.status_event_cursor = registry.status_revision; // 全候选周期覆盖到当前最新状态。
            break;
        }
        let offset = next.saturating_sub(oldest) as usize; // 环内序号连续，可O(1)直接索引。
        let Some(event) = registry.status_events.get(offset).copied() else {
            request_forced_candidate_poll(cache); // 防御饱和或异常缺口，同样失败关闭为全候选复验。
            cache.status_event_cursor = registry.status_revision; // 不重复卡在损坏事件槽。
            break;
        };
        let candidate_index = cache.candidate_indices.get(&event.entity).copied(); // 非当前候选事件O(1)跳过。
        if candidate_index.is_some() && *poll_budget == 0 {
            break; // 保留本事件到下一帧，绝不消费通知却未复验实体。
        }
        cache.status_event_cursor = event.revision; // 只有已跳过或已复验后才推进事件游标。
        event_budget -= 1; // 单帧消息处理同样服从固定预算。
        if let Some(candidate_index) = candidate_index {
            poll_candidate(
                cache,
                registry,
                candidate_index,
                scene_id,
                roots,
                asset_server,
                session_previews,
            ); // Replace observer精确唤醒Ready、Failed或状态移除根。
            *poll_budget -= 1; // 共享候选Query硬预算。
            polled += 1; // 测试可证明Changed只触碰具体实体。
        }
    }
    polled // 非候选状态事件不会计入候选查询数。
}

/// 按固定连续游标复验一次全候选周期。
#[allow(clippy::too_many_arguments)] // 与poll_candidate共享同一完整身份参数集。
pub(super) fn poll_forced_candidates(
    cache: &mut PlacementWaterSavePreflightCache, // runtime/GLB事件合并状态。
    registry: &PlacementWaterEvidenceRegistry,    // 每个候选只读取自身最近状态序号。
    scene_id: &SceneId,                               // 保存请求冻结场景。
    roots: &Query<
        (
            &ScenePlacementInstance,
            &DynamicWaterMeshOverrides,
            Option<&DynamicWaterMeshOverrideStatus>,
            &WorldAssetRoot,
            Option<&EditorPlacedAsset>,
        ),
        With<ScenePlacementWaterEvidence>,
    >, // 每项仍只使用Query::get。
    asset_server: &AssetServer,                   // 相关AssetEvent后的实际加载状态。
    session_previews: &BTreeMap<String, Entity>,  // gate和预览归属同步复验。
    poll_budget: &mut usize,                      // 与状态和Pending轮询共享预算。
) -> usize {
    let mut polled = 0; // 返回本路径实际实体读取次数。
    while cache.forced_poll_remaining > 0 && *poll_budget > 0 && !cache.candidates.is_empty() {
        let index = cache.forced_poll_cursor % cache.candidates.len(); // 固定候选数组内公平环绕。
        poll_candidate(
            cache,
            registry,
            index,
            scene_id,
            roots,
            asset_server,
            session_previews,
        );
        cache.forced_poll_cursor = (index + 1) % cache.candidates.len(); // 下一帧从本项之后继续。
        cache.forced_poll_remaining -= 1; // 一个候选只消费本周期一份责任。
        *poll_budget -= 1; // 所有直接实体读取共同服从硬预算。
        polled += 1; // 测试与诊断统计真实工作量。
    }
    polled
}

/// 稳定Waiting期间只公平轮询当前Pending候选。
#[allow(clippy::too_many_arguments)] // 与poll_candidate共享同一完整身份参数集。
pub(super) fn poll_pending_candidates(
    cache: &mut PlacementWaterSavePreflightCache, // Pending集合与公平下标游标。
    registry: &PlacementWaterEvidenceRegistry,    // Pending转Ready也必须使用该实体新状态信用。
    scene_id: &SceneId,                               // 保存请求冻结场景。
    roots: &Query<
        (
            &ScenePlacementInstance,
            &DynamicWaterMeshOverrides,
            Option<&DynamicWaterMeshOverrideStatus>,
            &WorldAssetRoot,
            Option<&EditorPlacedAsset>,
        ),
        With<ScenePlacementWaterEvidence>,
    >, // 只查询未决实体，不重扫Ready/Failed根。
    asset_server: &AssetServer, // 即使没有AssetEvent也可发现当前Pending句柄确定失败。
    session_previews: &BTreeMap<String, Entity>, // 预览被回收时下一次轮询撤销信用。
    poll_budget: &mut usize,    // 单帧总候选查询剩余额度。
) -> usize {
    let target = (*poll_budget).min(cache.pending_candidates.len()); // 同一Pending根一帧最多查询一次。
    let mut polled = 0; // 实际处理数随候选离开Pending仍保持有界。
    while polled < target && !cache.pending_candidates.is_empty() {
        let next = cache
            .pending_candidates
            .range((Excluded(cache.pending_poll_cursor), Unbounded))
            .next()
            .copied()
            .or_else(|| cache.pending_candidates.first().copied()); // 到末尾后从最小槽公平环绕。
        let Some(candidate_index) = next else {
            break; // 集合在上一次poll中清空时安全收口。
        };
        cache.pending_poll_cursor = candidate_index; // 即使本项转Ready也从它之后继续寻找。
        poll_candidate(
            cache,
            registry,
            candidate_index,
            scene_id,
            roots,
            asset_server,
            session_previews,
        );
        polled += 1; // 同一帧最多处理进入时Pending集合大小。
        *poll_budget -= 1; // target已经保证不会下溢。
    }
    polled
}
