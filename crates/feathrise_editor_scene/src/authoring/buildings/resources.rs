//! 建筑作者索引、分帧收集和单次后台校验的 ECS 资源。

use std::sync::Arc; // 主线程与校验任务共享不可变 BVH，不复制整图建筑几何。

use bevy::{ecs::system::SystemParam, prelude::*}; // Resource 与只读校验状态视图。
use bevy::tasks::Task; // 索引构建与点击校验各自最多一个在途任务。

use super::BuildingEditTarget;
use super::types::*; // 资源槽持有同域普通输入和结果类型。

/// 移动/旋转预览只读已有校验槽，不暴露建筑索引或后台任务的写权限。
#[derive(SystemParam)]
pub(crate) struct BuildingEditValidation<'w> {
    state: Option<Res<'w, BuildingAuthoringState>>,
}

impl BuildingEditValidation<'_> {
    pub(crate) fn pending(&self) -> bool {
        self.state
            .as_ref()
            .is_some_and(|state| state.validation_pending())
    }

    pub(crate) fn pending_for(&self, target: &BuildingEditTarget, world_xz: Vec2, yaw_degrees: f32) -> bool {
        self.state.as_ref().is_some_and(|state| {
            state.validation_pending()
                && state.validation_target.as_ref().is_some_and(|(accepted, position, yaw)| {
                    accepted == target && *position == world_xz && *yaw == yaw_degrees.rem_euclid(360.0)
                })
        }) // 只有本次冻结目标被受理才保持预览；拒绝、失败、迟到或成功采用均结束暂存。
    }
}

/// 三种建筑幽灵状态复用的固定半透明材质。
#[derive(Resource)]
pub(super) struct BuildingGhostMaterials {
    pub(super) pending: Handle<StandardMaterial>, // 尚未校验的琥珀色模型。
    pub(super) valid: Handle<StandardMaterial>,   // 已通过校验的绿色模型。
    pub(super) invalid: Handle<StandardMaterial>, // 已拒绝候选或索引失败的红色模型。
}

impl BuildingGhostMaterials {
    /// 返回当前Gizmo语义对应的共享材质句柄。
    pub(super) fn handle(&self, tone: BuildingPreviewTone) -> Handle<StandardMaterial> {
        match tone {
            BuildingPreviewTone::Pending => self.pending.clone(), // 等待态不猜测合法性。
            BuildingPreviewTone::Valid => self.valid.clone(),     // 成功态与绿色辅助线一致。
            BuildingPreviewTone::Invalid => self.invalid.clone(), // 失败态与红色辅助线一致。
        }
    }
}

/// 当前F12会话至多一个建筑模型幽灵和一份有界扫描队列。
#[derive(Resource, Default)]
pub(super) struct BuildingGhostPreviewState {
    pub(super) root: Option<Entity>, // 当前Scene0或primitive幽灵根。
    pub(super) key: Option<BuildingGhostAssetKey>, // 只有选择模型变化才替换根。
    pub(super) phase: BuildingGhostPhase, // 加载、隔离和显示采用明确有限状态。
    pub(super) scan: Option<BuildingGhostScan>, // Scene0就绪后按固定预算消费。
    pub(super) primitive_load: Option<BuildingGhostPrimitiveLoad>, // 静态单体就绪前不生成不完整Mesh3d。
    pub(super) tone: BuildingPreviewTone, // 新扫描节点和既有网格读取同一颜色。
    pub(super) desired_visible: bool,     // 光标离开视口时保留缓存根但立即隐藏。
}

impl BuildingGhostPreviewState {
    /// 释放根身份和全部未完成扫描；材质强句柄由独立固定资源继续复用。
    pub(super) fn clear(&mut self) {
        *self = Self::default(); // 下一次有效选择从完整空状态重新建立根。
    }
}

/// 当前 F12 建筑作者链的全部有界状态。
#[derive(Resource, Default)]
pub(in crate::authoring) struct BuildingAuthoringState {
    pub(super) desired_identity: Option<BuildingIndexIdentity>, // 当前草稿要求的联合身份。
    pub(super) gather: Option<BuildingIndexGather>, // 主线程固定预算收集的唯一最新快照。
    pub(super) index_task: Option<Task<BuildingIndexBuildResult>>, // CPU 池单在途 BVH 构建。
    pub(super) active_identity: Option<BuildingIndexIdentity>, // 已原子采用索引身份。
    pub(super) active_index: Option<Arc<BuildingSpatialIndex>>, // 点击任务 O(1) 克隆共享。
    pub(super) audit_error: Option<(BuildingIndexIdentity, String)>, // 当前完整索引的保存级首个非法原因。
    pub(super) index_error: Option<(BuildingIndexIdentity, String)>, // 同身份失败不在稳定帧重试。
    pub(super) next_ticket: u64, // 每次真实点击递增，零号保留未初始化。
    pub(super) validation_task: Option<Task<BuildingValidationTaskOutput>>, // 点击校验与整平共用 single-flight。
    pub(super) validation_target: Option<(BuildingEditTarget, Vec2, f32)>, // 已受理手柄的目标、XZ与角度；旧按钮、复制或另一姿态不能接管预览。
    pub(super) last_validation: Option<BuildingValidationResult>, // 红色非法原因与预览颜色回显。
    pub(super) validation_waiting: Option<BuildingValidationResult>, // 缺页或暂缺正式快照时保留唯一冻结点击。
    pub(super) audit_ground: Option<BuildingGroundQuery>, // 同一BVH的分页审计进度，不反复重建空间索引。
    pub(super) ground_content: Option<[u64; 7]>,          // 空间索引的地基审计来源，与相机无关。
    pub(super) audit_retirement: Option<(BuildingIndexIdentity, BuildingGroundQuery)>, // 取消的大点集移入原CPU槽释放，避免主线程遍历BTreeMap。
}

impl BuildingAuthoringState {
    pub(in crate::authoring) fn validation_pending(&self) -> bool {
        self.validation_task.is_some() || self.validation_waiting.is_some()
    }
    pub(super) fn retire_audit(&mut self) {
        if let Some(query) = self.audit_ground.take() {
            debug_assert!(self.audit_retirement.is_none()); // 下一批开始前必须已经把上一份移入worker。
            let identity = self
                .active_identity
                .as_ref()
                .or(self.desired_identity.as_ref())
                .expect("审计等待具有完整空间身份")
                .clone();
            self.audit_retirement = Some((identity, query));
        }
    }
    /// 返回指定联合身份能否进入场景保存事务。
    pub(super) fn save_readiness_for(
        &self,
        identity: &BuildingIndexIdentity,
    ) -> BuildingSaveReadiness {
        if let Some((_, error)) = self
            .index_error
            .as_ref()
            .filter(|(failed_identity, _)| failed_identity == identity)
        {
            return BuildingSaveReadiness::Failed(error.clone()); // 未知预设或坏几何不能当作空建筑放行。
        }
        if let Some((_, error)) = self
            .audit_error
            .as_ref()
            .filter(|(audited_identity, _)| audited_identity == identity)
        {
            return BuildingSaveReadiness::Failed(error.clone()); // 已确定非法建筑不能被来源重载或索引等待掩盖。
        }
        if self.desired_identity.as_ref() != Some(identity)
            || self.active_identity.as_ref() != Some(identity)
            || self.active_index.is_none()
        {
            return BuildingSaveReadiness::Waiting(
                "当前场景建筑空间索引与地基审计尚未完成".to_owned(),
            );
        }
        if self.audit_ground.is_some() || self.index_task.is_some() {
            return BuildingSaveReadiness::Waiting(
                "建筑地基正在等待精确高度页或后台审计".to_owned(),
            );
        }
        if self.validation_task.is_some() || self.validation_waiting.is_some() {
            return BuildingSaveReadiness::Waiting("一个建筑落点正在后台校验".to_owned()); // 保存不能先于已受理的作者点击冻结旧草稿。
        }
        BuildingSaveReadiness::Ready
    }
}
