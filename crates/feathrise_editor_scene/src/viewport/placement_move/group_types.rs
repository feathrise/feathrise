//! 整组变换的快照和后台结果；普通资产共用中心，完整身份失效则整组恢复。

use super::{math::MoveCamera, types::{MoveHandle, PatchSourceSnapshot}};
use crate::authoring::CourtyardBatchChange;
use crate::viewport::multi_select::SelectedMember;
use bevy::prelude::*;
use feathrise_scene_content::{SceneId, ScenePlacementRecord, TerrainObjectTransform};
use std::sync::Arc;

#[derive(PartialEq, Eq)]
pub(super) enum GroupPhase {
    Preparing,
    Dragging,
    Waiting,
    Finishing,
}

pub(super) struct GroupRoot {
    pub entity: Entity,
    pub pivot: Entity,
    pub index: usize,
    pub id: String,
    pub parent: Option<Entity>, // 正式根或作者根保留原来源关系，收尾恢复其完整局部姿态。
    pub pivot_parent: Option<Entity>, // 不同粘贴容器的作者根统一使用独立临时父级，不限制粘贴次数。
}

pub(super) struct GroupPivot {
    pub entity: Entity,
    pub parent: Option<Entity>, // 保持实际源/批次的显隐和退役所有权。
    pub attach: Entity, // 原根挂在恢复源框架的叶节点，而非丢弃源线性部分的公共世界节点。
    pub cleanup: Entity, // 辅助层级最高节点，先预算解绑全部原根再级联销毁。
}

pub(super) struct GroupSource {
    pub index: usize,
    pub before: ScenePlacementRecord,
    pub frame: TerrainObjectTransform,
    pub bounds: Rect, // patch使用所属源局部范围，不能拿场景容器裁掉合法洞口。
}

pub(super) struct GroupDrag {
    /// 原事务已正式采用的结束原因；清零临时pivot不能被外部投影误认成Esc取消。
    pub committed: bool,
    pub scene_id: SceneId,
    pub author_revision: u64,
    pub structure_revision: u64,
    pub terrain_revision: u64,
    pub derived_pose_revision: u64, // 准备、拖动和等待结果均冻结同一代已采用派生姿态。
    pub terrain_content: Option<[u64; 7]>,
    pub selection_revision: u64,
    pub members: Arc<Vec<SelectedMember>>,
    pub origin: Vec3,
    pub start_point: Vec3,
    pub start_cursor: Vec2,
    pub camera: MoveCamera,
    pub ui_size: Vec2,
    pub handle: MoveHandle,
    pub snapping: feathrise_editor_core::EditorSnapSettings,
    pub bounds: Rect,
    pub delta_min: Vec3,
    pub delta_max: Vec3,
    pub delta: Vec3,
    pub rotation_degrees: f32,
    pub factor: f32,
    pub min_factor: f32,
    pub max_factor: f32,
    pub pivots: Vec<GroupPivot>,
    pub phase: GroupPhase,
    pub sources: Vec<GroupSource>,
    pub terrain_sources: Vec<PatchSourceSnapshot>, // 按来源去重，实时身份门不再扫描整个选集。
    pub roots: Vec<GroupRoot>,
    pub prepared: usize,
    pub prefetch_cursor: usize,
    pub cleanup_cursor: usize,
    pub root_epoch: u64,
    pub pending_preview: Option<Entity>, // 准备过程中仅允许本手势刚补出的确切作者根完成一次Add登记。
    pub formal_runtime_identity: Option<(Option<SceneId>, u64, usize)>, // 正式批次的采用/退役不触发根Add/Remove，单独冻结公开采用身份。
}

impl GroupDrag {
    /// 预览、高度页需求、历史和高亮共用同一个仿射变换，禁止只旋转模型而漏转成员位置。
    pub(super) fn transformed_position(&self, position: Vec3) -> Vec3 {
        self.origin + self.delta
            + Quat::from_rotation_y(self.rotation_degrees.to_radians())
                * ((position - self.origin) * self.factor)
    }
}

/// 后台完整预检、地形偏移和堆容量计费后，主线程只交换已备好的记录。
pub(super) struct PreparedGroupMove {
    pub apply: Vec<(usize, ScenePlacementRecord)>,
    pub history: Option<CourtyardBatchChange>,
    pub retired: Vec<ScenePlacementRecord>, // 后台预留容量，采用时移出旧堆数据，随后仍在同一槽回收。
}
