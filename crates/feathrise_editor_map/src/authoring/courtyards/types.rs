//! 院落精确地基任务的不可变候选与分页进度；不持有ECS借用或额外任务槽。
use crate::authoring::buildings::{BuildingGroundQuery, CourtyardBuildingValidator};
use feathrise_map_content::{BuildingPresetDescriptor, MapPlacementRecord};
use std::collections::BTreeSet;

/// 冻结的整组待校验输入；最多256成员，只保存有限精确点及共享BVH。
pub(super) struct CourtyardGroundWork {
    pub(super) validator: CourtyardBuildingValidator,
    pub(super) candidates: Vec<(MapPlacementRecord, Option<BuildingPresetDescriptor>)>,
    pub(super) excluded_ids: BTreeSet<String>,
    pub(super) query: Option<BuildingGroundQuery>,
}
