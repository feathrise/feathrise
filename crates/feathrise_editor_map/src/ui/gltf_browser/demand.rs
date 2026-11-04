//! GLB浏览器的作者结构观察与展开资产需求纯计算。

#[cfg(test)]
use feathrise_map_content::{MapPlacementRecord}; // 纯签名等价测试仍复用公共绑定上限。

use super::BrowserSessionObservation; // 生产观察只读取O(1)作者结构元数据。
#[cfg(test)]
use super::{PlacementSignature, PlacementWaterBindingSignature}; // 测试完整快照不参与生产帧路径。
use crate::authoring::EditorAuthoringSession; // 作者会话提供独立且确定性的摆放结构代次。

/// 测试用完整结构提取；生产签名构建会按单项路径/字符串预算渐进复制水绑定。
#[cfg(test)]
pub(super) fn placement_structure_signature(record: &MapPlacementRecord) -> PlacementSignature {
    PlacementSignature {
        editor: record.editor.clone(),
        placement_id: record.id.clone(), // 稳定实例ID决定根行和展开偏好。
        asset_path: record.asset_path.clone(), // 只有来源路径变化才改变Scene0需求。
        asset_target: record.asset_target, // Scene0与直接primitive必须形成不同浏览结构。
        water_bindings: record
            .water_surfaces
            .iter()
            .map(PlacementWaterBindingSignature::from)
            .collect(), // 流速与高度不进入绑定签名，连续拖动不会重建全部摆放表。
    }
}

/// 以O(1)草稿元数据和作者显式结构代次构造当前观察。
pub(super) fn observe_browser_session(
    session: &EditorAuthoringSession, // 当前作者草稿与独立摆放结构修订。
) -> BrowserSessionObservation {
    let (draft_available, draft_map_id, source_placement_count) = session
        .draft
        .as_ref()
        .map(|draft| {
            (
                true,                              // Some草稿即使零摆放也是真实可浏览地图。
                Some(draft.map_id.to_string()),    // 换图清除跨图选择和展开状态。
                draft.placements.placements.len(), // 新增或删除先由O(1)长度识别。
            )
        })
        .unwrap_or((false, None, 0)); // 尚未载入草稿时没有任何路径需求。
    BrowserSessionObservation {
        scene_objects_revision: session.scene_objects_revision,
        draft_available,        // 同步真实空地图与等待草稿状态。
        draft_map_id,           // 保留稳定地图身份。
        source_placement_count, // 完整原始计数不受浏览上限截断。
        placement_structure_revision: session.placement_structure_revision, // 同图等长整体重载也由显式代次确定识别。
    }
}

/// 作者资源Changed后判断是否必须扫描并重建最多8192条完整浏览签名。
pub(super) fn session_change_requires_signature_refresh(
    previous: &BrowserSessionObservation, // 上一次已经同步到浏览器的作者观察。
    next: &BrowserSessionObservation,     // 当前帧只读取轻量元数据和历史尾得到的观察。
) -> bool {
    previous.draft_available != next.draft_available
        || previous.scene_objects_revision != next.scene_objects_revision
        || previous.draft_map_id != next.draft_map_id
        || previous.source_placement_count != next.source_placement_count
        || previous.placement_structure_revision != next.placement_structure_revision
    // 流速、高度、变换和其他作者域不会推进结构代次，因此Resource Changed也保持O(1)短路。
}

#[cfg(test)]
#[path = "demand/tests.rs"]
mod tests; // 显式结构代次与非结构数值修改边界由无ECS测试锁定。
