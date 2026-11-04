//! 整网格水证据完成缓存到保存入口结果的失败关闭转换。

use super::super::PlacementWaterSaveReadiness;
use crate::authoring::resources::{
    PlacementWaterCachedReadiness, PlacementWaterSavePreflightCache,
};

/// 把缓存内完成值转换为保存模块既有公开结果。
pub(crate) fn placement_water_cached_readiness(
    cache: &PlacementWaterSavePreflightCache, // 稳定帧只读取一个小型枚举。
) -> PlacementWaterSaveReadiness {
    if !cache.evidence_complete
        || !cache.retired_evidence.is_empty()
        || cache.forced_poll_remaining > 0
        || cache.scene_asset_invalidation_pending
    {
        return PlacementWaterSaveReadiness::Waiting(
            "正在分帧复验当前Scene0整网格水证据".to_owned(),
        ); // 即使调用方遗漏Advance.collecting门，也不能读取换代前的旧Ready。
    }
    match &cache.cached_readiness {
        PlacementWaterCachedReadiness::Collecting => {
            PlacementWaterSaveReadiness::Waiting("正在分帧收集当前Scene0整网格水证据".to_owned())
        } // 调用方通常用进度结构显示更具体的成员前缀。
        PlacementWaterCachedReadiness::Ready => PlacementWaterSaveReadiness::Ready, // 无额外分配。
        PlacementWaterCachedReadiness::Waiting(detail) => {
            PlacementWaterSaveReadiness::Waiting(detail.clone()) // 底栏拥有自己的状态字符串。
        }
        PlacementWaterCachedReadiness::Failed(detail) => {
            PlacementWaterSaveReadiness::Failed(detail.clone()) // 确定失败只在停止保存帧复制一次。
        }
    }
}
