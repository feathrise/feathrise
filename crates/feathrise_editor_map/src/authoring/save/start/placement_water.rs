//! 保存启动前的整网格水记录索引、根所有权筛选与完整Scene0证明。

mod evidence; // marker成员分帧聚合、定向状态唤醒与Pending实体轮询保持独立职责。
mod registry; // Add/Remove/Replace observer增量维护根表和固定容量状态事件环。

use std::collections::BTreeMap; // 最多256项非空水绑定按稳定placement ID建立小型根索引。

use bevy::prelude::Entity; // session预览必须用精确实体而非仅用同名ID认领。
use feathrise_map_content::{MapPlacementsAsset}; // 作者记录与公共全图水绑定预算共同限制缓存大小。
#[cfg(test)]
use feathrise_map_content::{MapId, MapPlacementRecord}; // 纯聚合oracle的冻结输入类型。
use feathrise_scene::MapPlacementInstance; // 根组件提供不可变来源快照与三态批次门。
#[cfg(test)]
use feathrise_scene::{DynamicWaterMeshOverrideStatus, DynamicWaterMeshOverrides}; // 纯聚合oracle只在回归测试读取状态与覆盖。

use crate::authoring::resources::PlacementWaterSavePreflightCache; // 保存single-flight资源持有跨等待帧的固定游标。

pub(super) use evidence::{
    advance_placement_water_evidence_cache, placement_water_cached_readiness,
}; // 保存入口只编排记录扫描、证据扫描和完成值。
pub(super) use registry::consume_relevant_world_asset_events; // 独立WorldAsset消息读者按固定预算只唤醒相关GLB。
pub(crate) use registry::{
    observe_added_placement_water_status, observe_removed_placement_water_status,
    observe_replaced_placement_water_status, register_placement_water_evidence_root,
    unregister_placement_water_evidence_root,
}; // 插件安装生命周期observer后，稳定帧不再扫描marker或状态Changed Query。

/// 每帧最多检查的普通placement记录数；非空结果另受全图256绑定硬门约束。
pub(super) const PLACEMENT_WATER_RECORD_SCAN_BUDGET_PER_FRAME: usize = 256;

/// 一帧推进非空水记录索引后的有界进度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PlacementWaterRecordScanProgress {
    pub(super) processed: usize, // 本帧真实读取的placement数量，完整缓存复用时必须为零。
    pub(super) scanned: usize,   // 当前作者修订已经检查到的记录总数。
    pub(super) total: usize,     // 当前完整摆放文档记录数，用于保存底栏反馈。
    pub(super) complete: bool,   // 只有完整扫描后才能断言没有遗漏后段非空水记录。
}

/// 当前草稿中全部非空整网格水体的运行时预检结果。
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PlacementWaterSaveReadiness {
    Ready,           // 每项都存在完整记录快照完全相等且已经Ready的合法根。
    Waiting(String), // 匹配根仍在等待World实例或有界动态水材质验证。
    Failed(String),  // 缺根、身份不一致或运行时已经给出明确失败。
}

/// 保存根的所有权类别；退役树和非本session预览没有枚举值，直接失败关闭。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PlacementWaterRootScope {
    ActiveFormal,   // 当前活动正式批次，物理Pending标记是否清完不影响身份。
    PendingFormal,  // 当前尚未采用的隐藏正式候选，可提供同快照Pending或Ready证据。
    SessionPreview, // 当前作者会话BTreeMap精确拥有的未保存预览实体。
}

/// 按固定记录预算建立当前修订的非空水placement小索引。
pub(super) fn advance_placement_water_record_cache(
    cache: &mut PlacementWaterSavePreflightCache,
    author_revision: u64,
    structure_revision: u64,
    placements: &MapPlacementsAsset,
) -> Result<PlacementWaterRecordScanProgress, String> {
    let same_input = cache.map_id.as_ref() == Some(&placements.map_id)
        && cache.author_revision == Some(author_revision)
        && cache.structure_revision == Some(structure_revision)
        && cache.total_records == placements.placements.len(); // 地图、作者修订与记录长度共同组成缓存输入身份。
    if !same_input {
        evidence::reset_placement_water_evidence_cache(cache); // 新作者快照不能复用旧根聚合或Pending轮询信用。
        evidence::retire_active_water_records(cache); // 旧记录及ID字符串O(1)移交，换代帧不再由Vec/BTreeMap::clear深析构。
        cache.map_id = Some(placements.map_id.clone()); // 新扫描首先冻结稳定地图身份。
        cache.author_revision = Some(author_revision); // 迟到旧修订缓存不能继续追加到新草稿。
        cache.structure_revision = Some(structure_revision); // 增删、换源或绑定身份变化显式冻结新结构代次。
        cache.next_record = 0; // 从文档首项重新开始固定预算扫描。
        cache.total_records = placements.placements.len(); // 状态反馈与防御性身份比较使用同一长度。
        cache.total_surface_count = 0; // 公共全图绑定预算随新修订重新累计。
        cache.complete = false; // 至少推进一次后才能断言空文档完整。
    }
    if cache.complete {
        return Ok(PlacementWaterRecordScanProgress {
            processed: 0,               // Pending等待帧复用已完成索引，不再读取8192条普通记录。
            scanned: cache.next_record, // 完整缓存游标应等于当前总数。
            total: cache.total_records, // 继续返回稳定诊断总量。
            complete: true,             // 调用方可立即进入最多256项根证明。
        });
    }
    let start = cache.next_record; // 冻结本帧切片起点供预算断言和测试读取。
    let end = start
        .saturating_add(PLACEMENT_WATER_RECORD_SCAN_BUDGET_PER_FRAME)
        .min(placements.placements.len()); // 即使下标异常也只截到当前文档末尾。
    for record in &placements.placements[start..end] {
        if record.water_surfaces.is_empty() {
            continue; // 普通placement不进入小型保存证明缓存。
        }
        cache.total_surface_count = cache
            .total_surface_count
            .checked_add(record.water_surfaces.len())
            .ok_or_else(|| "整网格水体保存前绑定数量计算溢出".to_owned())?; // 损坏输入失败关闭而不是绕过预算。
        let record_index = cache.water_records.len(); // 当前非空记录将稳定追加到此槽。
        if cache
            .water_record_indices
            .insert(record.id.clone(), record_index)
            .is_some()
        {
            return Err(format!("整网格水保存记录存在重复摆放ID：{}", record.id)); // 根证据不能猜测重复ID归属。
        }
        cache.water_records.push(record.clone()); // 非空记录数不大于绑定数，因此Vec最多256项。
    }
    cache.next_record = end; // 完成本帧固定切片后才发布精确游标。
    cache.complete = end == placements.placements.len(); // 最后一片完成后才允许根证明阶段。
    Ok(PlacementWaterRecordScanProgress {
        processed: end.saturating_sub(start), // 测试可证明单帧从不超过固定预算。
        scanned: end,                         // 底栏展示已经检查的完整前缀。
        total: placements.placements.len(),   // 总数来自本次同身份输入。
        complete: cache.complete,             // 完整扫描最早在末项被检查后发布。
    })
}

/// 判定一个根是否由当前正式批次/候选或当前session精确预览拥有。
pub(super) fn placement_water_root_scope(
    entity: Entity,
    instance: &MapPlacementInstance,
    is_editor_preview: bool,
    session_previews: &BTreeMap<String, Entity>,
) -> Option<PlacementWaterRootScope> {
    if is_editor_preview {
        return (instance.is_active()
            && session_previews.get(instance.id()).copied() == Some(entity))
        .then_some(PlacementWaterRootScope::SessionPreview); // 同ID但不在当前session映射中的残留预览不得提供证明。
    }
    if instance.is_active() {
        Some(PlacementWaterRootScope::ActiveFormal) // 只读共享门一次判定整批当前正式身份。
    } else if instance.is_pending() {
        Some(PlacementWaterRootScope::PendingFormal) // 三态门明确排除已经退役但物理标记尚未清理的旧根。
    } else {
        None // 换图或换代退役根即使同MapId、同ID、同水值也不能冒充当前证据。
    }
}

/// 保存不能只验证JSON字段；GLB节点、UV和材质必须先由实际World实例完整通过运行时预检。
#[cfg(test)]
pub(crate) fn placement_water_save_readiness<'a>(
    map_id: &MapId,
    water_records: &[MapPlacementRecord],
    roots: impl Iterator<
        Item = (
            &'a MapPlacementInstance,
            &'a DynamicWaterMeshOverrides,
            Option<&'a DynamicWaterMeshOverrideStatus>,
            bool, // 当前根Scene0或依赖是否已由AssetServer明确判定失败。
        ),
    >,
) -> PlacementWaterSaveReadiness {
    let mut roots_by_id = BTreeMap::<&str, Vec<_>>::new(); // 输入只含非空水证据根，规模与公共256绑定预算同阶。
    for (instance, overrides, status, scene_load_failed) in roots {
        roots_by_id.entry(instance.id()).or_default().push((
            instance,
            overrides,
            status,
            scene_load_failed,
        )); // 同ID正式根、候选和精确预览各自保留真实回执。
    }
    for record in water_records {
        let Some(candidates) = roots_by_id.get(record.id.as_str()) else {
            return PlacementWaterSaveReadiness::Failed(format!(
                "摆放{}没有可验证的当前正式根、候选或精确预览Scene0根",
                record.id,
            ));
        };
        let mut matched_snapshot = false; // 只有MapId与完整记录来源、变换和水数组全部相等才置位。
        let mut pending = false; // 任一匹配根仍可能成功时优先继续等待。
        let mut failure = None; // 所有匹配根均确定失败后才向作者发布具体诊断。
        for (instance, overrides, status, scene_load_failed) in candidates {
            if !instance.matches_record(map_id, record, overrides.surfaces()) {
                continue; // 跨图、旧资产、旧变换或旧水数组均不能证明当前草稿。
            }
            matched_snapshot = true; // 当前候选确实对应完整作者记录快照。
            match status {
                Some(status) if status.is_ready() => {
                    failure = None; // Ready证据优先于同快照旧根失败。
                    pending = false; // Ready证据也优先于同快照仍在途根。
                    break; // 任一精确快照Ready即可证明当前作者值可构建。
                }
                Some(status) if status.failure_detail().is_some() => {
                    failure.get_or_insert_with(|| {
                        status
                            .failure_detail()
                            .expect("分支已确认失败详情存在")
                            .to_owned()
                    }); // 若另一个同快照根Ready或Pending，后续循环仍可覆盖本结论。
                }
                Some(_) | None if *scene_load_failed => {
                    failure.get_or_insert_with(|| {
                        format!("Scene0或其依赖加载失败：{}", record.asset_path)
                    }); // WorldInstanceReady永远不会到达时不能继续伪装Pending。
                }
                Some(_) | None => pending = true, // AssetServer仍可收敛时保留自动保存请求。
            }
        }
        if !matched_snapshot {
            return PlacementWaterSaveReadiness::Failed(format!(
                "摆放{}当前根的地图、资产、变换或水体覆盖与草稿不一致",
                record.id,
            ));
        }
        if pending {
            return PlacementWaterSaveReadiness::Waiting(format!(
                "摆放{}仍在验证Scene0节点、UV与动态水材质",
                record.id,
            ));
        }
        if let Some(detail) = failure {
            return PlacementWaterSaveReadiness::Failed(format!(
                "摆放{}的整网格水体运行时验证失败：{detail}",
                record.id,
            )); // 同快照没有Ready或Pending时，才把全部确定失败的根报告给作者。
        }
    }
    PlacementWaterSaveReadiness::Ready // 空索引或全部非空水记录均有精确Ready证明。
}
