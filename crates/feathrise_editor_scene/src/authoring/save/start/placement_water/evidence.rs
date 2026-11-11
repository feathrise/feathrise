//! 保存请求冻结后的Scene0整网格水证据增量聚合与定向等待。

mod poll; // 状态事件、强制失效与稳定Pending的直接实体轮询独立维护固定预算。
mod readiness; // 完成缓存到保存模块结果的失败关闭转换保持独立。
mod retirement; // 旧成员代次容器O(1)移交后按固定元素预算析构。

use std::collections::BTreeMap; // 当前session预览所有权以稳定placement ID精确认领。

use bevy::prelude::*; // AssetServer、Entity与Query直接实体访问。
use bevy::world_serialization::{WorldAsset, WorldAssetRoot}; // 根强句柄只读加载状态，不发起新IO。
use feathrise_scene_content::SceneId; // 保存请求已经冻结的场景身份。
use feathrise_scene::{
    DynamicWaterMeshOverrideStatus, DynamicWaterMeshOverrides, ScenePlacementInstance,
    ScenePlacementRuntimePhase, ScenePlacementRuntimeStatus, ScenePlacementWaterEvidence,
}; // 精确根快照、三态gate和公开运行时换代共同组成保存证明。

use self::poll::{
    PLACEMENT_WATER_CANDIDATE_POLL_BUDGET_PER_FRAME, inspect_candidate_root,
    poll_changed_candidates, poll_forced_candidates, poll_pending_candidates,
}; // 三类后续轮询共享同一单帧实体查询预算。
use self::retirement::{
    PLACEMENT_WATER_RETIRE_BUDGET_PER_FRAME, drain_retired_evidence, retire_active_evidence,
}; // 成员换代帧不深析构候选、HashMap或失败字符串。
use crate::authoring::resources::{
    PlacementWaterCachedReadiness, PlacementWaterCandidateObservation,
    PlacementWaterEvidenceCandidate, PlacementWaterEvidenceRegistry, PlacementWaterRecordEvidence,
    PlacementWaterSavePreflightCache,
}; // 单次保存缓存保存全部有界游标与聚合计数。
use crate::components::EditorPlacedAsset;
pub(crate) use readiness::placement_water_cached_readiness; // 保存入口只读取完成枚举，不接触内部集合。 // 精确区分当前session预览与正式/候选根。


/// 单帧最多从登记表直接读取的证据根数量。
pub(super) const PLACEMENT_WATER_EVIDENCE_SCAN_BUDGET_PER_FRAME: usize = 64;

/// 一帧证据缓存推进量；测试与底栏可直接验证没有隐藏全量枚举。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PlacementWaterEvidenceAdvance {
    pub(crate) roots_processed: usize, // 本帧从marker登记表读取的实体数。
    pub(crate) roots_scanned: usize,   // 当前成员代次已经完成的根前缀。
    pub(crate) roots_total: usize,     // 当前成员代次冻结的根总数。
    pub(crate) candidates_polled: usize, // 本帧按Entity直接复验的候选数。
    pub(crate) collecting: bool,       // true时零候选还不能解释为确定缺根。
}

/// 作者修订换代复用证据模块的同一退役FIFO，不向保存入口暴露内部退役模块。
pub(super) fn retire_active_water_records(cache: &mut PlacementWaterSavePreflightCache) {
    self::retirement::retire_active_water_records(cache); // 旧记录和ID索引只移动所有权，后续由固定预算析构。
}

/// 清除旧成员代次的证据聚合，但保留冻结作者记录与其已分配容量。
pub(crate) fn reset_placement_water_evidence_cache(
    cache: &mut PlacementWaterSavePreflightCache, // 记录扫描和证据扫描共享同一保存single-flight缓存。
) {
    cache.evidence_registry_revision = None; // 下一次推进必须重新冻结成员表代次。
    cache.evidence_total_roots = 0; // 旧总量不能参与新场景或新根集合诊断。
    cache.next_evidence_root = 0; // 新成员表从首项固定切片开始。
    cache.evidence_complete = false; // 完成前不发布缺根或Ready结论。
    retire_active_evidence(cache); // O(1)移交全部可能随候选数增长的容器，下一帧起定额回收。
    cache.pending_poll_cursor = 0; // 公平轮询从新候选首项开始。
    cache.status_event_cursor = 0; // 新成员代次完成时从登记表当前末尾开始追踪。
    cache.forced_poll_cursor = 0; // 清除旧runtime/GLB复验游标。
    cache.forced_poll_remaining = 0; // 当前没有完整候选集合可供强制复验。
    cache.runtime_scene_id = None; // 完成聚合时再冻结正式运行时身份。
    cache.runtime_phase_tag = None; // 旧phase不能阻止新候选第一次复验。
    cache.runtime_revision = 0; // 新聚合完成后写入当前已采用修订。
    cache.runtime_attempt_revision = 0; // 新聚合完成后写入当前尝试修订。
    cache.cached_readiness = PlacementWaterCachedReadiness::Collecting; // 稳定结果显式回到处理中。
}

/// 把公开运行时phase压成缓存内的稳定轻量标签。
fn placement_runtime_phase_tag(phase: ScenePlacementRuntimePhase) -> u8 {
    match phase {
        ScenePlacementRuntimePhase::Waiting => 0, // 正式文件或Scene依赖仍在等待。
        ScenePlacementRuntimePhase::Building => 1, // 新隐藏候选可能改变gate归属。
        ScenePlacementRuntimePhase::Ready => 2,   // 新批次已经活动，旧批次应已退役。
        ScenePlacementRuntimePhase::Failed => 3,  // 本轮候选已取消，Pending根可能退役。
    }
}

/// 判断正式运行时的语义身份是否已变化，并冻结最新值。
fn observe_runtime_identity_change(
    cache: &mut PlacementWaterSavePreflightCache, // 保存缓存持有上次已经复验的正式语义。
    runtime: &ScenePlacementRuntimeStatus, // gate原子值不触发Changed，采用/失败修订负责公开唤醒。
) -> bool {
    let phase_tag = placement_runtime_phase_tag(runtime.phase); // 枚举标签避免资源层依赖业务方法。
    let changed = cache.runtime_scene_id != runtime.scene_id
        || cache.runtime_phase_tag != Some(phase_tag)
        || cache.runtime_revision != runtime.revision
        || cache.runtime_attempt_revision != runtime.attempt_revision; // 进度心跳不改变所有权，不触发全候选复验。
    cache.runtime_scene_id = runtime.scene_id.clone(); // 跨场景即使修订数字相同也必须被识别。
    cache.runtime_phase_tag = Some(phase_tag); // phase换代覆盖候选取消但未采用的路径。
    cache.runtime_revision = runtime.revision; // 活动批次严格采用值。
    cache.runtime_attempt_revision = runtime.attempt_revision; // 新失败或重试候选值。
    changed // 调用方只在真实语义改变时排入全候选固定预算周期。
}

/// 更新一个Scene0资产ID的候选引用计数。
fn add_scene_asset_reference(
    cache: &mut PlacementWaterSavePreflightCache, // 单次保存只维护候选实际引用的去重ID。
    asset_id: bevy::asset::AssetId<WorldAsset>,   // 类型化ID不会与其他资产类别碰撞。
) {
    *cache.scene_asset_counts.entry(asset_id).or_default() += 1; // 候选硬上限保证usize不会溢出。
}

/// 候选根换Scene0句柄时回收旧资产ID引用。
fn remove_scene_asset_reference(
    cache: &mut PlacementWaterSavePreflightCache, // 引用计数归零后相关旧GLB事件不再唤醒。
    asset_id: bevy::asset::AssetId<WorldAsset>,   // 候选原先冻结的Scene0 ID。
) {
    let Some(count) = cache.scene_asset_counts.get_mut(&asset_id) else {
        return; // 防御迟到重复更新，不允许下溢。
    };
    *count = count.saturating_sub(1); // 候选每次句柄变化只释放一份引用。
    if *count == 0 {
        cache.scene_asset_counts.remove(&asset_id); // 保持事件命中集合只含当前候选。
    }
}

/// 候选根切换Scene0句柄时同步资产索引，并撤销只属于旧句柄的状态代际门。
fn rebind_candidate_scene_asset(
    cache: &mut PlacementWaterSavePreflightCache, // 候选、引用计数与未授信总数必须同批更新。
    candidate_index: usize,                       // 当前直接实体查询对应的稳定候选槽。
    scene_asset_id: bevy::asset::AssetId<WorldAsset>, // 本次查询读取到的最新强句柄ID。
) {
    let old_asset_id = cache.candidates[candidate_index].scene_asset_id; // 先复制旧ID以结束字段借用。
    if old_asset_id == scene_asset_id {
        return; // 稳定句柄不改引用计数，也保留当前代际门。
    }
    remove_scene_asset_reference(cache, old_asset_id); // 旧GLB事件从当前候选引用集合撤销。
    add_scene_asset_reference(cache, scene_asset_id); // 新GLB事件从这一帧起可以定向失效候选。
    let candidate = &mut cache.candidates[candidate_index]; // 独占候选字段发布新句柄身份。
    candidate.scene_asset_id = scene_asset_id; // 之后所有强制轮询都解释新Scene0。
    candidate.last_status_revision = 0; // 旧句柄上的状态事件不能授信新Scene0。
    candidate.required_status_revision = None; // 旧句柄的下限不能转嫁到新Scene0，随后按新ID重新建立门。
}

/// 对一次真实根观察应用Scene0资产事件代际门，旧Ready只能退化为Pending而不能启动IO。
pub(super) fn gate_candidate_observation(
    cache: &mut PlacementWaterSavePreflightCache, // 保存single-flight持有资产下限和候选实体级信用。
    candidate_index: usize,                       // 只修改发生定向轮询的一个候选。
    scene_asset_id: Option<bevy::asset::AssetId<WorldAsset>>, // 根仍存在时同步最新Scene0句柄。
    status_revision: Option<u64>, // registry只提供该实体已经真实发生的最近状态序号。
    observation: PlacementWaterCandidateObservation, // 当前组件、加载状态与完整根身份的联合观察。
) -> PlacementWaterCandidateObservation {
    if let Some(scene_asset_id) = scene_asset_id {
        rebind_candidate_scene_asset(cache, candidate_index, scene_asset_id); // 先绑定正确资产再读取其失效下限。
    }
    if let Some(status_revision) = status_revision {
        cache.candidates[candidate_index].last_status_revision = cache.candidates[candidate_index]
            .last_status_revision
            .max(status_revision); // 只单调接纳该实体自己的状态事件，不受其他根推进影响。
    }
    let asset_id = cache.candidates[candidate_index].scene_asset_id; // 句柄变更已经在上方完成。
    if let Some(floor) = cache.scene_asset_status_floors.get(&asset_id).copied()
        && cache.candidates[candidate_index].last_status_revision <= floor
    {
        let candidate = &mut cache.candidates[candidate_index]; // 同一候选只保留最新资产事件下限。
        candidate.required_status_revision = Some(
            candidate
                .required_status_revision
                .map_or(floor, |required| required.max(floor)),
        ); // 连续Modified只抬高下限，不重复增加等待计数。
    }
    if matches!(observation, PlacementWaterCandidateObservation::Ineligible) {
        cache.candidates[candidate_index].required_status_revision = None; // 退役或错快照根由Missing/其他候选裁决。
        return observation; // 无资格根绝不被伪装成可收敛Pending证据。
    }
    let status_gate_satisfied = cache.candidates[candidate_index]
        .required_status_revision
        .is_some_and(|required| cache.candidates[candidate_index].last_status_revision > required); // 必须严格晚于AssetEvent到达时冻结的状态尾部。
    if status_gate_satisfied {
        cache.candidates[candidate_index].required_status_revision = None; // 新Pending或新Ready状态已经明确属于热重载之后。
    }
    if cache.candidates[candidate_index]
        .required_status_revision
        .is_some()
    {
        PlacementWaterCandidateObservation::Pending // 重新读取到的旧Ready或旧Failed都不能越过代际门。
    } else {
        observation // 新状态事件已授信，继续使用真实Pending/Ready/Failed观察。
    }
}

/// 从一条记录的聚合计数中移除旧候选观察。
fn remove_candidate_observation(
    evidence: &mut PlacementWaterRecordEvidence, // 只修改候选所属的唯一记录。
    entity: Entity,                              // Failed诊断以实体为稳定键精确回收。
    observation: &PlacementWaterCandidateObservation, // 候选槽中的旧分类。
) {
    match observation {
        PlacementWaterCandidateObservation::Ineligible => {} // 无资格根没有进入任何聚合计数。
        PlacementWaterCandidateObservation::Pending => {
            evidence.pending_count = evidence.pending_count.saturating_sub(1); // 状态离开Pending时撤销等待信用。
        }
        PlacementWaterCandidateObservation::Ready => {
            evidence.ready_count = evidence.ready_count.saturating_sub(1); // Ready根退役后不能继续证明保存。
        }
        PlacementWaterCandidateObservation::Failed(_) => {
            evidence.failures.remove(&entity); // 另一候选仍可能Pending或Ready。
        }
    }
}

/// 把候选新观察加入所属记录的聚合计数。
fn add_candidate_observation(
    evidence: &mut PlacementWaterRecordEvidence, // 每条记录最多拥有工程上限内候选。
    entity: Entity,                              // 失败详情稳定排序与移除共用此键。
    observation: &PlacementWaterCandidateObservation, // 当前真实根观察。
) {
    match observation {
        PlacementWaterCandidateObservation::Ineligible => {} // 失去所有权或快照不符即无证明。
        PlacementWaterCandidateObservation::Pending => {
            evidence.pending_count += 1; // 候选硬上限保证计数安全。
        }
        PlacementWaterCandidateObservation::Ready => {
            evidence.ready_count += 1; // 任一Ready会在记录分类时优先采用。
        }
        PlacementWaterCandidateObservation::Failed(detail) => {
            evidence.failures.insert(entity, detail.clone()); // 只为确定失败保留作者可读细节。
        }
    }
}

/// 重新派生一条记录的Ready/Waiting/Failed/Missing分类。
fn refresh_record_category(cache: &mut PlacementWaterSavePreflightCache, record_index: usize) {
    cache.missing_records.remove(&record_index); // 四类集合互斥，先清理旧派生值。
    cache.waiting_records.remove(&record_index); // 候选变化只触碰所属记录。
    cache.failed_records.remove(&record_index); // Ready记录不需要单独集合即可O(1)判定全体完成。
    let evidence = &cache.record_evidence[record_index]; // 与冻结water_records严格等长。
    if evidence.ready_count > 0 {
        return; // Ready优先于同快照旧失败或仍在途候选。
    }
    if evidence.pending_count > 0 {
        cache.waiting_records.insert(record_index); // 仍可能收敛时不能提前失败。
    } else if !evidence.failures.is_empty() {
        cache.failed_records.insert(record_index); // 全部精确候选确定失败才发布失败。
    } else {
        cache.missing_records.insert(record_index); // 无当前正式、候选或精确预览根。
    }
}

/// 只从三个小型分类集合首项重建全局结果，不线性重验全部记录。
fn refresh_cached_readiness(cache: &mut PlacementWaterSavePreflightCache) {
    if !cache.evidence_complete {
        cache.cached_readiness = PlacementWaterCachedReadiness::Collecting; // 完整成员前缀未完成时保持处理中。
        return;
    }
    let first_missing = cache.missing_records.first().copied(); // 每个集合最多256项。
    let first_waiting = cache.waiting_records.first().copied(); // BTreeSet首项O(logN)内稳定取得。
    let first_failed = cache.failed_records.first().copied(); // 不遍历完整记录数组。
    let first_problem = [first_missing, first_waiting, first_failed]
        .into_iter()
        .flatten()
        .min(); // 三个Option的固定三项比较保持原记录顺序语义。
    cache.cached_readiness = match first_problem {
        None => PlacementWaterCachedReadiness::Ready, // 所有记录隐式属于Ready分类。
        Some(index) if first_waiting == Some(index) => {
            PlacementWaterCachedReadiness::Waiting(format!(
                "摆放{}仍在验证Scene0节点、UV与动态水材质",
                cache.water_records[index].id,
            ))
        }
        Some(index) if first_failed == Some(index) => {
            let detail = cache.record_evidence[index]
                .failures
                .first_key_value()
                .map(|(_, detail)| detail.as_str())
                .unwrap_or("运行时未提供失败详情"); // 分类与详情表理论上同源，缺失时仍失败关闭。
            PlacementWaterCachedReadiness::Failed(format!(
                "摆放{}的整网格水体运行时验证失败：{detail}",
                cache.water_records[index].id,
            ))
        }
        Some(index) => PlacementWaterCachedReadiness::Failed(format!(
            "摆放{}没有可验证的当前正式根、候选或精确预览Scene0根",
            cache.water_records[index].id,
        )), // 剩余最早问题只能属于Missing集合。
    };
}

/// 替换一个候选观察，并只更新所属记录与Pending轮询集合。
pub(super) fn update_candidate(
    cache: &mut PlacementWaterSavePreflightCache, // 所有聚合计数与候选槽在同一函数内保持一致。
    candidate_index: usize,                       // 由反向索引或固定游标取得。
    observation: PlacementWaterCandidateObservation, // 本次真实根观察。
    scene_asset_id: Option<bevy::asset::AssetId<WorldAsset>>, // 根缺失时保留旧ID只会产生一次无害额外唤醒。
) {
    let entity = cache.candidates[candidate_index].entity; // 失败详情和反向索引共用稳定Entity。
    let record_index = cache.candidates[candidate_index].record_index; // 只触碰唯一记录聚合。
    let observation_unchanged = cache.candidates[candidate_index].observation == observation; // 稳定Pending轮询复用既有分类和诊断字符串。
    if let Some(scene_asset_id) = scene_asset_id {
        rebind_candidate_scene_asset(cache, candidate_index, scene_asset_id); // 非轮询测试入口同样保持资产索引一致。
    }
    if observation_unchanged {
        return; // 状态和归属未变时不改计数、不重建全局文字；稳定Waiting只承担一次直接实体读取。
    }
    let old = std::mem::replace(
        &mut cache.candidates[candidate_index].observation,
        observation,
    ); // 先取得旧分类，之后计数减一与加一组成原子缓存更新。
    remove_candidate_observation(&mut cache.record_evidence[record_index], entity, &old); // 撤销旧聚合。
    let current = &cache.candidates[candidate_index].observation; // 借用刚发布的新分类。
    add_candidate_observation(&mut cache.record_evidence[record_index], entity, current); // 应用新聚合。
    if matches!(current, PlacementWaterCandidateObservation::Pending) {
        cache.pending_candidates.insert(candidate_index); // 稳定Waiting公平轮询本槽。
    } else {
        cache.pending_candidates.remove(&candidate_index); // Ready、Failed或退役根只由事件再次唤醒。
    }
    if cache.evidence_complete {
        refresh_record_category(cache, record_index); // 完成聚合后候选变化只重派生本条记录。
        refresh_cached_readiness(cache); // 三个集合首项固定成本重建全局结果。
    }
}

/// 把一个首次命中的精确同快照候选加入有界聚合。
fn add_candidate(
    cache: &mut PlacementWaterSavePreflightCache, // 当前证据成员代次的唯一聚合写侧。
    entity: Entity,                               // marker登记表提供的精确根身份。
    record_index: usize,                          // 根稳定ID已经O(log256)定位冻结记录。
    scene_asset_id: bevy::asset::AssetId<WorldAsset>, // 当前强Scene0句柄身份。
    status_revision: u64, // marker根最近一次真实状态事件序号，用于成员重建后恢复新生命周期信用。
    observation: PlacementWaterCandidateObservation, // 首次状态、加载与gate联合观察。
) -> Result<(), String> {
    if cache.candidate_indices.contains_key(&entity) {
        return Ok(()); // 登记表不应重复，但幂等保护阻止重复计数。
    }
    cache.candidates.try_reserve(1).map_err(|e| format!("水绑定保存证据内存分配失败：{e}"))?;
    let candidate_index = cache.candidates.len(); // 连续槽供状态事件O(1)定位。
    cache.candidates.push(PlacementWaterEvidenceCandidate {
        entity,                                                      // 保存精确实体代次。
        record_index,                                                // 指向冻结记录数组。
        scene_asset_id,                                              // 相关资产事件索引。
        last_status_revision: status_revision, // 只冻结当前实体自己的最近状态序号。
        required_status_revision: None, // 若同资产已有失效下限，统一门函数会立即建立等待责任。
        observation: PlacementWaterCandidateObservation::Ineligible, // 通过统一更新函数加入聚合计数。
    });
    cache.candidate_indices.insert(entity, candidate_index); // 状态observer事件可以直接命中。
    add_scene_asset_reference(cache, scene_asset_id); // Scene0引用计数与候选同批发布。
    let observation = gate_candidate_observation(
        cache,
        candidate_index,
        None,
        Some(status_revision),
        observation,
    ); // 成员扫描与后续轮询共用同一AssetEvent代际门。
    update_candidate(cache, candidate_index, observation, None); // 首次聚合复用同一计数逻辑。
    Ok(())
}

/// 固定当前正式运行时与状态事件尾部，完整派生全部记录类别。
fn finish_evidence_collection(
    cache: &mut PlacementWaterSavePreflightCache, // 当前成员表已读取到冻结总量末尾。
    registry: &PlacementWaterEvidenceRegistry,    // 记录完成时的最新状态事件序号。
    runtime: &ScenePlacementRuntimeStatus,          // gate语义基线从本帧公开证据冻结。
) {
    cache.evidence_complete = true; // 从这一行之后零候选才可以解释为Missing。
    cache.status_event_cursor = registry.status_revision; // 首次观察已读取当前状态，不重放历史事件环。
    let runtime_changed = observe_runtime_identity_change(cache, runtime); // 多帧扫描期间换代必须复验早先候选。
    cache.missing_records.clear(); // 防御重复完成调用，类别从当前计数完整重建。
    cache.waiting_records.clear(); // 每条记录只进入一个集合或隐式Ready。
    cache.failed_records.clear(); // 旧诊断不能跨成员代次残留。
    for index in 0..cache.water_records.len() {
        refresh_record_category(cache, index); // 公共记录硬上限256，且只在成员聚合完成一次执行。
    }
    refresh_cached_readiness(cache); // 后续稳定帧直接复用此完成值。
    if runtime_changed || cache.scene_asset_invalidation_pending {
        cache.scene_asset_invalidation_pending = false; // 失效责任已经转成完整候选复验游标。
        request_forced_candidate_poll(cache); // 全周期完成前Advance继续报告collecting，旧Ready不能启动IO。
    }
}

/// 开始或重建一个marker成员代次的有界证据聚合。
fn ensure_evidence_generation(
    cache: &mut PlacementWaterSavePreflightCache, // 作者记录已经完整冻结。
    registry: &PlacementWaterEvidenceRegistry,    // observer维护的连续实体表及成员代次。
    runtime: &ScenePlacementRuntimeStatus,          // 空水记录也冻结当前语义供一致调试。
) -> bool {
    if cache.evidence_registry_revision == Some(registry.membership_revision) {
        return true; // 同一成员代次继续现有游标或稳定结果。
    }
    reset_placement_water_evidence_cache(cache); // 根增删立即撤销旧Ready聚合。
    if !cache.retired_evidence.is_empty() {
        return false; // 旧代次尚未按预算析构完前不建立第二份大候选容器。
    }
    cache.evidence_registry_revision = Some(registry.membership_revision); // 新游标只解释当前连续表。
    cache.evidence_total_roots = registry.entities.len(); // 冻结固定索引终点。
    cache.record_evidence = (0..cache.water_records.len())
        .map(|_| PlacementWaterRecordEvidence::default())
        .collect(); // 最多256个小型计数对象，成员聚合只分配一次。
    let _ = observe_runtime_identity_change(cache, runtime); // 在首根扫描前冻结gate/修订，识别跨帧扫描中的原子采用。
    if cache.water_records.is_empty() {
        cache.evidence_complete = true; // 没有整网格水绑定时不枚举任何根。
        cache.cached_readiness = PlacementWaterCachedReadiness::Ready; // 普通8192 placement保存保持O(1)。
        cache.status_event_cursor = registry.status_revision; // 忽略与本保存无关的历史状态事件。
        return true;
    }
    if registry.overflowed || registry.total_count != registry.entities.len() {
        cache.evidence_complete = true; // 异常爆量不继续解释被截断登记表。
        cache.scene_asset_invalidation_pending = false; // 没有完整成员表可复验，硬上限失败优先结束保存。
        cache.cached_readiness = PlacementWaterCachedReadiness::Failed(
            "整网格水保存证据内存分配失败或登记身份不完整".to_owned());
    }
    true
}

/// 把runtime/gate或相关GLB变化合并成一个全候选有界复验周期。
pub(super) fn request_forced_candidate_poll(cache: &mut PlacementWaterSavePreflightCache) {
    if cache.candidates.is_empty() {
        return; // 没有候选时成员聚合已经给出Missing，无需空轮询。
    }
    cache.forced_poll_remaining = cache.forced_poll_remaining.max(cache.candidates.len()); // 连续事件不重置游标或无限延后已完成前缀。
}

/// 推进首次证据枚举或后续定向等待。
#[allow(clippy::too_many_arguments, clippy::type_complexity)] // 保存证明必须在一个系统快照内读取完整身份。
pub(crate) fn advance_placement_water_evidence_cache(
    cache: &mut PlacementWaterSavePreflightCache, // 保存请求冻结作者记录和全部跨帧游标。
    registry: &PlacementWaterEvidenceRegistry,    // observer维护的marker成员与状态事件。
    runtime: &ScenePlacementRuntimeStatus,          // 正式gate语义变化唤醒全候选复验。
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
    >, // 只按registry/candidate中的Entity直接读取。
    asset_server: &AssetServer,                   // 只读Scene0及递归依赖加载失败。
    session_previews: &BTreeMap<String, Entity>,  // 当前作者会话精确预览所有权。
) -> PlacementWaterEvidenceAdvance {
    if !drain_retired_evidence(cache, PLACEMENT_WATER_RETIRE_BUDGET_PER_FRAME)
        || !ensure_evidence_generation(cache, registry, runtime)
    {
        return PlacementWaterEvidenceAdvance {
            roots_processed: 0, // 退役期间不读取新成员，避免同时持有两份最大候选代次。
            roots_scanned: 0,
            roots_total: registry.total_count,
            candidates_polled: 0,
            collecting: true, // 下一帧继续定额析构，完成后冻结最新membership_revision。
        };
    }
    if matches!(
        cache.cached_readiness,
        PlacementWaterCachedReadiness::Failed(_)
    ) && cache.evidence_complete
        && (registry.overflowed || registry.total_count != registry.entities.len())
    {
        return PlacementWaterEvidenceAdvance {
            roots_processed: 0,                // 爆量失败稳定帧不继续读取截断登记表。
            roots_scanned: 0,                  // 没有一份可声明完整的成员前缀。
            roots_total: registry.total_count, // 底栏显示真实异常根量。
            candidates_polled: 0,              // 确定失败不自动重试。
            collecting: false,                 // 调用方应停止保存请求。
        };
    }
    if !cache.evidence_complete {
        let start = cache.next_evidence_root; // 冻结本帧成员切片起点。
        let end = start
            .saturating_add(PLACEMENT_WATER_EVIDENCE_SCAN_BUDGET_PER_FRAME)
            .min(cache.evidence_total_roots); // 固定根预算且不越过冻结总量。
        for entity in registry.entities[start..end].iter().copied() {
            let Ok((instance, _, _, _, _)) = roots.get(entity) else {
                continue; // 同帧迟到移除由下一帧membership_revision重建聚合。
            };
            let Some(&record_index) = cache.water_record_indices.get(instance.id()) else {
                continue; // 8192个普通或其他水记录根不进入本保存候选数组。
            };
            let Some((observation, asset_id)) = inspect_candidate_root(
                entity,
                scene_id,
                &cache.water_records[record_index],
                roots,
                asset_server,
                session_previews,
            ) else {
                continue; // 退役、跨场景、错快照或非本session预览不提供证明。
            };
            let status_revision = registry
                .status_revisions_by_entity
                .get(&entity)
                .copied()
                .unwrap_or_default(); // 没有状态事件时从零开始，任何资产下限都会保守阻断旧Ready。
            if let Err(detail) = add_candidate(
                cache,
                entity,
                record_index,
                asset_id,
                status_revision,
                observation,
            ) {
                cache.evidence_complete = true; // 硬上限失败不继续截断聚合。
                cache.cached_readiness = PlacementWaterCachedReadiness::Failed(detail); // 调用方零IO停止。
                break;
            }
        }
        cache.next_evidence_root = end; // 完成本帧固定切片后才发布游标。
        if !cache.evidence_complete && end == cache.evidence_total_roots {
            finish_evidence_collection(cache, registry, runtime); // 全部成员完成后一次派生最多256条记录分类。
        }
        return PlacementWaterEvidenceAdvance {
            roots_processed: end.saturating_sub(start), // 严格不超过64项。
            roots_scanned: end,                         // 底栏可展示当前成员前缀。
            roots_total: cache.evidence_total_roots,    // 同一成员代次冻结总量。
            candidates_polled: 0,                       // 首次观察计入根扫描，不重复计候选轮询。
            collecting: !cache.evidence_complete || cache.forced_poll_remaining > 0, // 跨扫描换代必须先完成复验。
        };
    }
    if observe_runtime_identity_change(cache, runtime) || cache.scene_asset_invalidation_pending {
        cache.scene_asset_invalidation_pending = false; // 只有真正排入完整候选周期后才消费锁存失效。
        request_forced_candidate_poll(cache); // gate/Scene0语义变化按候选数组固定游标复验。
    }
    let mut poll_budget = PLACEMENT_WATER_CANDIDATE_POLL_BUDGET_PER_FRAME; // 三条路径共享唯一Query硬预算。
    let mut candidates_polled = poll_changed_candidates(
        cache,
        registry,
        scene_id,
        roots,
        asset_server,
        session_previews,
        &mut poll_budget,
    ); // Added/Replace/Remove状态优先精确唤醒对应实体。
    candidates_polled += poll_forced_candidates(
        cache,
        registry,
        scene_id,
        roots,
        asset_server,
        session_previews,
        &mut poll_budget,
    ); // 事件环缺口、runtime gate或相关GLB事件渐进复验全部候选。
    if matches!(
        cache.cached_readiness,
        PlacementWaterCachedReadiness::Waiting(_)
    ) {
        candidates_polled += poll_pending_candidates(
            cache,
            registry,
            scene_id,
            roots,
            asset_server,
            session_previews,
            &mut poll_budget,
        ); // 稳定Pending只直接查询当前未决实体，不重建任何全局映射。
    }
    PlacementWaterEvidenceAdvance {
        roots_processed: 0, // 完成聚合后的稳定帧永远不重新枚举marker根。
        roots_scanned: cache.evidence_total_roots, // 完整成员前缀继续作为诊断。
        roots_total: cache.evidence_total_roots, // 成员变化会在下一帧重建新总量。
        candidates_polled,  // 总数严格不超过共享32项预算。
        collecting: cache.forced_poll_remaining > 0, // 全候选失效周期完成前不暴露旧Ready信用。
    }
}

#[cfg(test)]
mod tests; // 事件唤醒、稳定Pending预算与marker生命周期使用无渲染App回归。
