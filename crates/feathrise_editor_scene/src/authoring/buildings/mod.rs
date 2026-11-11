//! F12 建筑实例的有界空间索引、后台落点校验、原子采用与视口辅助线。
//!
//! 【功能设计】主线程按固定预算从当前 placements 收集建筑语义，CPU 池构建不可变 BVH；
//! 点击只提交一个 single-flight 校验任务，完成后同时核对场景、作者、结构、目录与票据身份。

use feathrise_scene_core::residency::SceneResidencyAppExt;

mod courtyard; // 院落整批操作复用同代BVH、五点地基和双向门前净空校验。
mod ghost; // 半透明模型幽灵按固定预算隔离Scene0并只进入F12专用渲染层。
mod ground; // 五点地基、院落和保存审计共用有限Mip0点集，缺页自动续接。
mod index; // 分帧收集建筑实例并在后台构建不可变 BVH。
mod leveling; // 显式建筑联合整平在后台克隆并修改真实命中层的GLB顶点。
mod terrain_coordinates; // 整平写侧冻结实际作者对象，将世界占地投到同一源坐标。
mod preview; // 占地、门前净空和正门箭头只绘制到 F12 专用 Gizmo 层。
mod resources; // 唯一索引与校验任务资源，保持明确并发上限。
mod snapping; // 普通网格与显式地面连接点吸附共用唯一候选姿态解析。
mod types; // 后台输入、结果、BVH 节点与纯空间查询普通类型。
mod validation; // 点击请求、五点地基检查和成功后的作者事务采用。
#[cfg(test)]
pub(crate) mod move_test_support; // 手柄回归复用真实校验与采用系统。
pub(in crate::authoring) use validation::apply_sparse_building_transaction;

use bevy::prelude::*; // App、Update 与系统集注册。
use feathrise_scene_content::BuildingPresetRegistry; // 保存门按同一目录修订建立联合身份。
use feathrise_scene_content::SceneId; // 已有建筑请求冻结场景稳定身份。

use crate::SceneEditorSystemSet;
use crate::authoring::EditorAuthoringSession; // 保存门读取当前场景与作者修订。 // 建筑任务和表现遵循编辑器统一阶段。

/// 文档切换只观察作者点击是否收口，不等待可撤销的后台空间索引或幽灵预览。
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct BuildingDocumentGate<'w> {
    state: Option<Res<'w, resources::BuildingAuthoringState>>,
}

impl BuildingDocumentGate<'_> {
    pub(crate) fn busy(&self) -> bool {
        self.state
            .as_ref()
            .is_some_and(|state| state.validation_pending())
    }
}

/// 原生窗口退出复用建筑请求的已有受理门；可重建的只读索引和幽灵预览无需阻塞。
pub(crate) fn exit_pending(world: &World) -> bool {
    world.get_resource::<resources::BuildingAuthoringState>()
        .is_some_and(|state| state.validation_pending())
}

pub(in crate::authoring) use courtyard::CourtyardBuildingValidator;
pub(in crate::authoring) use courtyard::{courtyard_validator_snapshot, validate_courtyard_batch}; // 院落后台只接收完整不可变空间快照。
pub(in crate::authoring) use ground::content_is_current;
pub(crate) use ground::controls_identity as clipmap_controls_identity;
pub(crate) use ground::cursor_target as clipmap_cursor_target;
pub(crate) use resources::BuildingEditValidation; // 移动模型暂存只读原校验槽的完整目标身份。
pub(super) use resources::BuildingAuthoringState; // 保存系统只读同一有界任务资源。
pub(in crate::authoring) use types::BuildingGroundQuery;
pub(super) use types::BuildingSaveReadiness; // 保存入口消费明确Ready/Waiting/Failed三态。

/// UI按钮或视口点击提交的已有建筑受检操作目标。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BuildingEditTarget {
    pub(crate) scene_id: SceneId,           // 迟到请求不能跨场景采用。
    pub(crate) placement_id: String,    // 当前建筑稳定实例ID。
    pub(crate) record_index: usize,     // 当前结构修订下的O(1)数组槽。
    pub(crate) structure_revision: u64, // 增删任何placement后请求失效。
    pub(crate) author_revision: u64,    // 其他作者操作优先于旧按钮或旧点击。
}

/// 视口点击时冻结的真实地形表层身份；联合整平不能在任务启动帧重新猜测命中层。
#[derive(Clone, Copy, Debug)]
pub(crate) struct BuildingTerrainHit {
    pub(crate) position: Vec3, // 射线与当前GLB三角的完整三维交点。
    pub(crate) triangle: u32,  // 当前拓扑中的原始三角序号。
}

/// 普通属性面板只发送意图；建筑域在同一后台校验门内决定是否修改作者数据。
#[derive(Message, Clone, Debug)]
pub(crate) enum BuildingEditRequest {
    Relocate {
        target: BuildingEditTarget,              // 移动前完整作者身份。
        world_xz: Vec2,                          // 视口真实地形命中的平面位置。
        terrain_hit: Option<BuildingTerrainHit>, // 显式整平时冻结洞穴真实表层。
    },
    RotateQuarterTurn {
        target: BuildingEditTarget, // 使用当前根位置并顺时针旋转一档。
    },
    RotateTo {
        target: BuildingEditTarget,
        yaw_degrees: f32, // 旋转手柄一次指定90°档位，逆时针/跨多档仍通过同一后台槽和一次历史。
    },
    DuplicateAt {
        target: BuildingEditTarget, // 源建筑保持不动且不预先创建重叠副本。
        world_xz: Vec2,             // 新副本通过校验后才原子出现。
        terrain_hit: Option<BuildingTerrainHit>, // 复制与移动使用同一命中身份协议。
    },
}

/// 返回当前草稿中的建筑能否安全进入任何文件写入。
pub(super) fn save_readiness(
    session: &EditorAuthoringSession,
    registry: Option<&BuildingPresetRegistry>,
    state: Option<&BuildingAuthoringState>,
    surface: Option<&feathrise_scene::DynamicClipmapGroundSurface>,
) -> BuildingSaveReadiness {
    let Some(draft) = session.draft.as_ref() else {
        return BuildingSaveReadiness::Ready; // 无草稿由保存入口自己的加载门诊断。
    };
    if state
        .is_some_and(|state| state.validation_task.is_some() || state.validation_waiting.is_some())
    {
        return BuildingSaveReadiness::Waiting("建筑点击正在等待精确地基或后台校验".to_owned());
    }
    if !draft
        .placements
        .placements
        .iter()
        .any(|record| record.building_preset_id.is_some())
    {
        return BuildingSaveReadiness::Ready; // 普通placement无需等待建筑目录或BVH。
    }
    let Some(registry) = registry else {
        return BuildingSaveReadiness::Failed("建筑预设目录资源未初始化".to_owned());
    };
    let Some(identity) = index::current_identity(session, registry) else {
        return BuildingSaveReadiness::Waiting("建筑预设目录尚未完成首次加载".to_owned());
    };
    let Some(state) = state else {
        return BuildingSaveReadiness::Failed("建筑校验系统未初始化".to_owned());
    };
    match state.save_readiness_for(&identity) {
        BuildingSaveReadiness::Ready => {}
        readiness => return readiness, // 当前索引或审计的确定错误优先，不能被旧来源票据改写成等待。
    }
    // 只有当前场景真实建筑的完整审计才拥有来源信用。空/旧索引不能阻止没有建筑的网格保存。
    match ground::content_is_current(state.ground_content, draft, surface, session.terrain_revision) {
        Ok(true) => BuildingSaveReadiness::Ready,
        Ok(false) => BuildingSaveReadiness::Waiting(
            "当前建筑地基的真实地形来源尚未完整采用，正在等待同修订审计".into(),
        ),
        Err(detail) => BuildingSaveReadiness::Waiting(format!(
            "建筑地基来源已换代，正在重建当前空间索引与审计：{detail}"
        )),
    }
}

/// 安装建筑作者索引、后台校验和视口预览。
pub(crate) fn install(app: &mut App) {
    app.add_systems(
        Update,
        crate::authoring::poll_sparse_building_history
            .in_set(SceneEditorSystemSet::Tasks)
            .after(crate::authoring::poll_sparse_terrain_task)
            .before(crate::authoring::start_sparse_terrain_save)
            .before(crate::authoring::start_draft_save),
    );
    app.init_resident_resource::<resources::BuildingAuthoringState>() // 全部大工作保持单任务和单快照上限。
        .init_resident_resource::<resources::BuildingGhostPreviewState>() // 当前会话至多缓存一个隐藏或可见模型根。
        .add_message::<BuildingEditRequest>() // 普通变换UI不得绕过建筑专用后台规则。
        .add_observer(ghost::observe_building_ghost_world_ready) // Scene0完整生成后才启动专层隔离。
        .add_systems(Startup, ghost::setup_building_ghost_materials) // 三态共享材质只在启动时登记一次。
        .add_systems(
            Update,
            (
                index::poll_index_build,     // 先释放已完成旧任务的 single-flight 槽。
                index::advance_index_gather, // 每帧最多收集固定数量 placement。
                index::sync_vegetation_clearance_override, // 只发布已完整采用BVH，失效时恢复正式净空。
                validation::poll_validation,               // 完整身份有效时一次性写入作者记录。
                validation::request_existing_edit, // 消费上一帧UI意图并复用同一single-flight槽。
                ghost::advance_building_ghost_scan, // 每帧最多隔离128个隐藏Scene0节点。
                ghost::advance_building_ghost_primitive, // 静态单体完整就绪并通过蒙皮门后才生成。
            )
                .chain()
                .in_set(SceneEditorSystemSet::Tasks),
        )
        .add_systems(
            Update,
            (
                validation::request_from_viewport, // 左键冻结当前建筑、网格、朝向与索引身份。
                ghost::sync_building_ghost,        // 模型只在选择变化时重建，稳定帧只同步根姿态。
                preview::draw_building_preview,    // 同帧显示占地、门前净空和正门方向。
            )
                .chain()
                .in_set(SceneEditorSystemSet::Presentation)
                .run_if(crate::scene_residency::document_active),
        );
}
