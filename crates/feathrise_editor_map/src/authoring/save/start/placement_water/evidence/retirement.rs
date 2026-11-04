//! 已失效整网格水证据代次的O(1)移交与固定预算析构。

use crate::authoring::resources::{
    PlacementWaterRetiredEvidenceGeneration, PlacementWaterSavePreflightCache,
}; // 只负责活动/退役容器的所有权转移，不解释业务证据。

/// 单帧最多释放的旧候选、失败详情或集合节点数量。
pub(super) const PLACEMENT_WATER_RETIRE_BUDGET_PER_FRAME: usize = 64;

/// 把当前活动容器O(1)移交给退役代次；存在退役工作时不会建立新的活动代次。
pub(super) fn retire_active_evidence(cache: &mut PlacementWaterSavePreflightCache) {
    let generation = PlacementWaterRetiredEvidenceGeneration {
        candidates: std::mem::take(&mut cache.candidates),
        candidate_indices: std::mem::take(&mut cache.candidate_indices).into_iter(),
        record_evidence: std::mem::take(&mut cache.record_evidence),
        pending_candidates: std::mem::take(&mut cache.pending_candidates),
        missing_records: std::mem::take(&mut cache.missing_records),
        waiting_records: std::mem::take(&mut cache.waiting_records),
        failed_records: std::mem::take(&mut cache.failed_records),
        scene_asset_counts: std::mem::take(&mut cache.scene_asset_counts).into_iter(),
        scene_asset_status_floors: std::mem::take(&mut cache.scene_asset_status_floors).into_iter(),
        water_records: Vec::new(), // 作者记录只在作者修订换代时由专用入口移交。
        water_record_indices: std::collections::BTreeMap::new().into_iter(), // 空拥有型游标保持统一退役结构。
    }; // 每个容器只移动三字指针/表头，成员变化帧不遍历4096个候选。
    if generation.is_empty() {
        return; // 首次保存或已清空代次无需建立空退役对象。
    }
    cache.retired_evidence.push_back(Box::new(generation)); // O(1)排队；正常状态机始终至多保留一个代次。
}

/// 作者修订换代时O(1)移交旧非空水记录与ID索引，避免`clear`同步深析构字符串和路径。
pub(super) fn retire_active_water_records(cache: &mut PlacementWaterSavePreflightCache) {
    let generation = PlacementWaterRetiredEvidenceGeneration {
        water_records: std::mem::take(&mut cache.water_records), // 完整记录Vec只交换表头，不在保存启动帧逐项drop。
        water_record_indices: std::mem::take(&mut cache.water_record_indices).into_iter(), // BTreeMap节点改由固定预算拥有型游标释放。
        ..Default::default()
    }; // 其余证据容器保持空值，避免再次移动刚刚建立的新代次。
    if generation.is_empty() {
        return; // 首次扫描没有旧作者记录需要退役。
    }
    cache.retired_evidence.push_back(Box::new(generation)); // 与证据代次共用同一FIFO和元素预算。
}

/// 固定预算释放旧代次；返回true表示本帧后可以建立最新成员代次。
pub(super) fn drain_retired_evidence(
    cache: &mut PlacementWaterSavePreflightCache,
    mut budget: usize,
) -> bool {
    let Some(retired) = cache.retired_evidence.front_mut() else {
        return true;
    };
    while budget > 0 {
        if let Some(candidate) = retired.candidates.pop() {
            drop(candidate); // Failed字符串也只在这一份预算内析构。
            budget -= 1;
            continue;
        }
        if let Some(record) = retired.record_evidence.last_mut() {
            if record.failures.pop_first().is_some() {
                budget -= 1; // 聚合表中的第二份失败诊断同样逐条释放。
                continue;
            }
            retired.record_evidence.pop(); // 空计数对象析构为常量工作。
            budget -= 1;
            continue;
        }
        if retired.candidate_indices.next().is_some() {
            budget -= 1;
            continue;
        }
        if retired.pending_candidates.pop_first().is_some()
            || retired.missing_records.pop_first().is_some()
            || retired.waiting_records.pop_first().is_some()
            || retired.failed_records.pop_first().is_some()
        {
            budget -= 1; // 四个有序集合共享同一元素预算。
            continue;
        }
        if retired.scene_asset_counts.next().is_some()
            || retired.scene_asset_status_floors.next().is_some()
            || retired.water_record_indices.next().is_some()
        {
            budget -= 1;
            continue;
        }
        if let Some(record) = retired.water_records.pop() {
            drop(record); // 旧记录最多256项并逐项释放；后续可再把单记录内部路径拆成多维预算。
            budget -= 1;
            continue;
        }
        break;
    }
    let complete = retired.is_empty();
    if complete {
        cache.retired_evidence.pop_front(); // 空容器只释放桶内存，不再深析构元素。
    }
    cache.retired_evidence.is_empty()
}

#[cfg(test)]
mod tests {
    use bevy::asset::{AssetId, uuid::Uuid}; // 互异Scene0 ID构造不依赖AssetServer注册。
    use bevy::prelude::*;
    use bevy::world_serialization::WorldAsset;
    use feathrise_map_content::MapPlacementRecord; // 作者记录退役回归保留真实字符串所有权。

    use super::*;
    use crate::authoring::resources::{
        PlacementWaterCandidateObservation, PlacementWaterEvidenceCandidate,
    };

    #[test]
    fn reset_moves_large_generation_then_releases_only_the_fixed_budget() {
        let mut cache = PlacementWaterSavePreflightCache::default();
        let scene_asset_id = Handle::<WorldAsset>::default().id();
        for index in 0..128 {
            let entity = Entity::from_bits((index + 1) as u64);
            cache.candidate_indices.insert(entity, index);
            cache.pending_candidates.insert(index);
            cache.candidates.push(PlacementWaterEvidenceCandidate {
                entity,
                record_index: 0,
                scene_asset_id,
                last_status_revision: 0, // 退役测试不需要新生命周期信用。
                required_status_revision: None, // 所有候选仅验证析构预算。
                observation: PlacementWaterCandidateObservation::Failed(format!(
                    "旧失败详情-{index}"
                )),
            });
        }

        super::super::reset_placement_water_evidence_cache(&mut cache);

        assert!(cache.candidates.is_empty());
        assert_eq!(cache.retired_evidence.len(), 1);
        assert_eq!(cache.retired_evidence[0].candidates.len(), 128);
        assert!(!drain_retired_evidence(
            &mut cache,
            PLACEMENT_WATER_RETIRE_BUDGET_PER_FRAME,
        ));
        assert_eq!(cache.retired_evidence[0].candidates.len(), 64); // 一帧只析构64个候选及其失败字符串。
        assert_eq!(cache.retired_evidence[0].candidate_indices.len(), 128); // Copy反向索引由后续拥有型游标独立计入预算。
        assert_eq!(cache.retired_evidence[0].pending_candidates.len(), 128); // 有序集合节点同样不再藏在候选预算内释放。
    }

    #[test]
    fn retirement_drains_four_thousand_distinct_assets_with_owned_constant_step_iterator() {
        let mut cache = PlacementWaterSavePreflightCache::default(); // 构造工程硬上限规模的互异Scene0资产索引。
        for index in 0..4_096_u128 {
            let asset_id = AssetId::<WorldAsset>::Uuid {
                uuid: Uuid::from_u128(index + 1), // 每项使用独立稳定ID，不能被HashMap合并。
            };
            cache.scene_asset_counts.insert(asset_id, 1); // 模拟最坏候选资产去重表。
        }

        super::super::reset_placement_water_evidence_cache(&mut cache); // 换代只把HashMap转成拥有型IntoIter。

        assert_eq!(cache.retired_evidence.len(), 1); // 完整索引仍由唯一退役代持有。
        assert_eq!(cache.retired_evidence[0].scene_asset_counts.len(), 4_096); // 转交帧没有提前遍历任何桶。
        assert!(!drain_retired_evidence(
            &mut cache,
            PLACEMENT_WATER_RETIRE_BUDGET_PER_FRAME
        )); // 首帧只推进固定64项。
        assert_eq!(cache.retired_evidence[0].scene_asset_counts.len(), 4_032); // 拥有型游标不会每项从HashMap首桶重新扫描。
    }

    #[test]
    fn author_revision_moves_water_records_without_clearing_owned_strings() {
        let mut cache = PlacementWaterSavePreflightCache::default(); // 模拟已经完成的上一作者修订记录缓存。
        cache.water_records.push(MapPlacementRecord {
            mesh_lod: None,
            terrain_binding: None,
            editor: Default::default(),
            terrain_patch: None,
            id: "旧河面".to_owned(), // 稳定ID拥有堆字符串，能暴露同步clear深析构回归。
            asset_path: "gltf/旧河面.glb".to_owned(), // 资产路径同样随完整记录移交。
            asset_target: Default::default(), // 退役夹具保持历史Scene0目标。
            building_preset_id: None, // 退役所有权测试不附带建筑字符串。
            courtyard: None,         // 独立摆放记录不属于任何院落副本。
            translation: [0.0, 0.0, 0.0], // 位姿标量不影响所有权测试。
            height_mode: feathrise_map_content::MapPlacementHeightMode::FixedHeight, // 所有权测试不访问地形。
            terrain_offset: 0.0,        // 固定高度不携带离地偏移。
            yaw_degrees: 0.0,           // 位姿标量不影响所有权测试。
            scale: [1.0, 1.0, 1.0],     // 位姿标量不影响所有权测试。
            water_surfaces: Vec::new(), // 本测试只锁定记录和ID字符串的转交边界。
        });
        cache.water_record_indices.insert("旧河面".to_owned(), 0); // BTreeMap键拥有第二份字符串。

        retire_active_water_records(&mut cache); // 换代入口只能移动Vec/BTreeMap所有权。

        assert!(cache.water_records.is_empty()); // 活动缓存已经可以立即建立新修订。
        assert!(cache.water_record_indices.is_empty()); // 旧ID不会混入新索引。
        assert_eq!(cache.retired_evidence.len(), 1); // 旧值仍由退役代持有而非当场析构。
        assert_eq!(cache.retired_evidence[0].water_records.len(), 1); // 完整记录等待固定预算。
        assert_eq!(cache.retired_evidence[0].water_record_indices.len(), 1); // ID节点等待固定预算。
    }
}
