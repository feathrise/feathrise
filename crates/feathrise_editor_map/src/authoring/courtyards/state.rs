//! 院落作者分帧索引及两个互斥后台工作槽。

use std::collections::{BTreeMap, VecDeque}; // 索引分帧建立，ECS 根同步同样使用固定预算队列。

use super::types::CourtyardGroundWork;
use bevy::prelude::*; // Resource 和地图身份。
use bevy::tasks::Task; // IO/CPU 完成后均由主线程非阻塞轮询。
use feathrise_map_content::{MapId, MapPlacementRecord}; // 索引仅保存槽位，操作冻结小型成员快照。

use crate::authoring::EditorAuthoringSession; // 草稿结构修订是索引唯一有效性依据。
use crate::ui::courtyard::CourtyardViewportMode; // 取消视口模式后旧落点不得迟到采用。
use crate::ui::placement_transform::sync_placement_record_world_with_terrain; // 定向活动根或补具有真实地形父级的作者预览。
use crate::ui::placement_water::PlacementInstanceIndex; // 稳定 ID 只查对应正式根。
use feathrise_scene::MapPlacementInstance; // ECS 根重建后仍核对当前活动身份。

const INDEX_BUDGET_PER_FRAME: usize = 128; // 单帧只读取固定数量摆放，避免大图全表扫描。
const VISUAL_SYNC_BUDGET_PER_FRAME: usize = 16; // 每帧消费16个稳定ID队列项；同ID并存代次仍定向合成，不代表16次World写入。

/// 保存目录时冻结原地图选择；热采用后才把原选中项组成第一份副本。
pub(super) struct CatalogAdoption {
    pub(super) baseline_revision: u64,
    pub(super) preset_id: String,
    pub(super) map_id: MapId,
    pub(super) author_revision: u64,
    pub(super) structure_revision: u64,
    pub(super) selected: Vec<(usize, MapPlacementRecord)>,
    pub(super) origin: Vec3,
    pub(super) yaw_degrees: f32,
}

/// 唯一目录写槽；外部文件可能变化，写任务从磁盘重新读取并追加。
pub(super) enum CatalogTask {
    Writing {
        task: Task<Result<(), String>>,
        adoption: CatalogAdoption,
    },
    AwaitAdoption(CatalogAdoption),
}

/// 地图数组索引只依赖摆放结构修订，纯变换不必反复扫描。
#[derive(Default)]
pub(super) struct CourtyardSelectionIndex {
    pub(super) map_id: Option<MapId>,
    pub(super) structure_revision: u64,
    pub(super) cursor: usize,
    pub(super) ready: bool,
    pub(super) by_placement_id: BTreeMap<String, usize>,
    pub(super) by_instance_id: BTreeMap<String, Vec<usize>>,
}

impl CourtyardSelectionIndex {
    /// 仅在完整分帧收集且身份匹配时提供 O(log n) 定位。
    pub(super) fn ready_for(&self, session: &EditorAuthoringSession) -> bool {
        self.ready
            && session
                .draft
                .as_ref()
                .is_some_and(|draft| self.map_id.as_ref() == Some(&draft.map_id))
            && self.structure_revision == session.placement_structure_revision
    }
}

/// 一次后台几何校验的输入身份；任何作者或目录换代都丢弃旧完成值。
#[derive(Clone)]
pub(super) struct OperationIdentity {
    pub(super) map_id: MapId,
    pub(super) author_revision: u64,
    pub(super) terrain_revision: u64,
    pub(super) ground_content: Option<[u64; 7]>,
    pub(super) structure_revision: u64,
    pub(super) building_revision: u64,
    pub(super) courtyard_revision: u64,
    pub(super) ticket: u64,
}

/// 新落印、移动、旋转和复制共用一个结果协议。
pub(super) struct OperationResult {
    pub(super) identity: OperationIdentity,
    pub(super) source: Vec<(usize, MapPlacementRecord)>,
    pub(super) creates_new: bool,
    pub(super) result: Result<Vec<MapPlacementRecord>, String>,
    pub(super) description: String,
    pub(super) required_mode: Option<CourtyardViewportMode>,
    pub(super) ground_work: Option<CourtyardGroundWork>, // 精确页等待复用原CPU槽，完整结果前无作者副作用。
    pub(super) input_target: Option<crate::viewport::vegetation_ground::GroundCursorTarget>, // 精确点击接管后仍核对模板、朝向与副本。
}

/// 一份成员作者记录的待同步世界状态。
struct PendingVisualRecord {
    index: usize,               // 防止后续数组变化使迟到视觉任务误指另一个成员。
    structure_revision: u64,    // 其他作者删除前缀时等待分帧索引换代后重新定位稳定 ID。
    record: MapPlacementRecord, // 完整期望值用于拒绝覆盖更新后的局部编辑。
    visible: bool,              // 撤销新增需要隐藏旧正式根，其余情况显示目标值。
}

/// 一张地图的分帧世界同步队列。
struct PendingVisualSync {
    map_id: MapId,                        // 切图后整队丢弃，绝不污染另一地图。
    items: VecDeque<PendingVisualRecord>, // 单批受摆放上限约束，多个历史动作按发生顺序追加。
}

/// 所有院落工作保持最多一个 CPU 校验、一个目录 IO 事务和一条视觉队列。
#[derive(Resource, Default)]
pub(crate) struct CourtyardAuthoringState {
    pub(super) index: CourtyardSelectionIndex,
    pub(super) catalog_task: Option<CatalogTask>,
    pub(super) operation_task: Option<Task<OperationResult>>,
    pub(super) operation_waiting: Option<OperationResult>,
    pub(super) ticket: u64,
    visual_sync: Option<PendingVisualSync>, // 操作/历史采用后只分帧同步ECS画面。
}

impl CourtyardAuthoringState {
    pub(crate) fn operation_pending(&self) -> bool {
        self.operation_task.is_some() || self.operation_waiting.is_some()
    }
    /// 模板写入和热采用还会给原位置成员成组；地图保存必须等这笔作者历史完成。
    /// 已完成作者记录的纯视觉队列不占保存门，避免为模型显示同步永久等待。
    pub(crate) fn save_pending(&self) -> bool {
        self.operation_pending() || self.catalog_task.is_some()
    }
    /// 完成全部可见根前，新的整组操作和历史命令保持请求以提供固定背压。
    pub(crate) fn visual_sync_pending(&self) -> bool {
        self.visual_sync
            .as_ref()
            .is_some_and(|pending| !pending.items.is_empty())
    }
}

#[cfg(test)]
#[path = "save_tests.rs"]
mod save_tests; // 普通模型院落的目录热采用与正式地图保存门回归。

#[cfg(test)]
mod queue_liveness_tests; // 单成员来源失效或等待不能丢弃整条有限视觉队列。

/// 作者数据及历史已采用后入队；完整记录复核确保迟到任务不会覆盖单成员新编辑。
pub(crate) fn queue_visual_sync(
    state: &mut CourtyardAuthoringState,
    session: &EditorAuthoringSession,
    updates: Vec<(usize, MapPlacementRecord, bool)>,
) {
    let Some(draft) = session.draft.as_ref() else {
        return;
    }; // 无活动地图无需保留世界任务。
    if state
        .visual_sync
        .as_ref()
        .is_some_and(|pending| pending.map_id != draft.map_id)
    {
        state.visual_sync = None; // 上张图的队列不能跨图应用。
    }
    let queue = state.visual_sync.get_or_insert_with(|| PendingVisualSync {
        map_id: draft.map_id.clone(),
        items: VecDeque::new(),
    });
    queue.items.extend(
        updates
            .into_iter()
            .map(|(index, record, visible)| PendingVisualRecord {
                index,
                structure_revision: session.placement_structure_revision,
                record,
                visible,
            }),
    ); // 新的撤销/重做意图追加，旧项仍按作者发生顺序检验。
}

/// 每帧最多消费16个ID队列项；旧地图、旧成员或已被个人编辑的记录自动跳过。
#[allow(clippy::too_many_arguments)]
pub(crate) fn sync_visuals(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut session: ResMut<EditorAuthoringSession>,
    root_index: Res<PlacementInstanceIndex>,
    roots: Query<(Entity, &MapPlacementInstance)>,
    mut state: ResMut<CourtyardAuthoringState>,
    terrain_queries: feathrise_scene::TerrainObjectQueries,
) {
    for _ in 0..VISUAL_SYNC_BUDGET_PER_FRAME {
        let Some(pending) = state.visual_sync.as_mut() else {
            return;
        };
        let Some(item) = pending.items.pop_front() else {
            state.visual_sync = None; // 已完成的队列不占用稳定帧资源。
            return;
        };
        let pending_map_id = pending.map_id.clone(); // NLL 结束队列借用，下面可读取分帧索引。
        let Some(draft) = session.draft.as_ref() else {
            state.visual_sync = None;
            return;
        };
        if draft.map_id != pending_map_id {
            state.visual_sync = None; // 切图时整批丢弃。
            return;
        }
        let still_valid = if item.structure_revision == session.placement_structure_revision {
            let current = draft.placements.placements.get(item.index);
            if item.visible {
                current == Some(&item.record)
            } else {
                current.is_none_or(|record| record.id != item.record.id)
            } // 同一结构下原槽位仍有精确意义。
        } else if state.index.ready_for(&session) {
            let current = state
                .index
                .by_placement_id
                .get(&item.record.id)
                .and_then(|index| draft.placements.placements.get(*index));
            if item.visible {
                current == Some(&item.record)
            } else {
                current.is_none()
            } // 结构换代只信新完整索引。
        } else {
            state
                .visual_sync
                .as_mut()
                .expect("本轮已有队列")
                .items
                .push_front(item); // 等分帧索引完整再重定位，不丢失未显示成员。
            return;
        };
        if !still_valid {
            continue;
        }
        crate::authoring::placement_deletion::reconcile_identity(
            &mut commands, &pending_map_id, &item.record.id,
        ); // 删除与恢复都只合成当前有效ID，整组已有根不得在采用帧产生无界ECS回调。
        if !item.visible {
            // 作者删除已采用；迟到清理只依赖地图和稳定ID，不能再被旧来源或姿态拒绝。
            // 删除票据已随作者事务整批发布，迟到视觉清理不能再Insert Hidden污染待归还原值。
            if let Some(entity) = session.placement_previews.remove(&item.record.id)
                && roots.get(entity).is_ok_and(|(_, instance)| {
                    instance.map_id() == &pending_map_id && instance.id() == item.record.id
                })
            {
                commands.entity(entity).try_despawn();
            } // 先隐藏再回收自有预览，不改正式姿态，也不把旧地址当作外部实体的所有权。
            continue;
        }
        let bounds = match crate::authoring::placement_record_bounds(draft, &item.record) {
            Ok(bounds) => bounds,
            Err(error) => {
                warn!("院落成员 {} 的来源已失效，跳过旧画面同步：{error}", item.record.id);
                continue; // 单成员失效不能丢弃后续已经采用作者数据的有效成员。
            }
        };
        if item.record.terrain_patch.is_some()
            && let Err(error) = super::super::edit::patch_preview_parent(draft, &terrain_queries, &item.record)
        {
            warn!("三维地形 {} 的预览父级尚未就绪：{error}", item.record.id);
            state.visual_sync.as_mut().expect("本轮已有队列").items.push_front(item);
            return; // 保留有限队列重试，不把源局部记录先暴露到世界identity下。
        }
        if let Err(error) = sync_placement_record_world_with_terrain(
            &mut commands,
            &asset_server,
            &mut session,
            &root_index,
            &roots,
            &item.record,
            bounds,
            true,
            true,
            &terrain_queries,
        ) {
            warn!("院落成员 {} 的分帧画面同步失败：{error}", item.record.id); // 作者数据和历史已完整采用。
        }
    }
}

/// 按稳定结构修订每帧收集有限记录，同时建立单成员和整组定位。
pub(super) fn advance_selection_index(
    session: Res<EditorAuthoringSession>, // 只读当前地图草稿与结构身份。
    mut state: ResMut<CourtyardAuthoringState>, // 单一分帧游标。
) {
    let Some(draft) = session.draft.as_ref() else {
        state.index = CourtyardSelectionIndex::default(); // 退出地图时释放旧身份。
        return;
    };
    if state.index.map_id.as_ref() != Some(&draft.map_id)
        || state.index.structure_revision != session.placement_structure_revision
    {
        state.index = CourtyardSelectionIndex {
            map_id: Some(draft.map_id.clone()),
            structure_revision: session.placement_structure_revision,
            ..default()
        }; // 新地图或成员归属变化后从头分帧建索引。
    }
    if state.index.ready {
        return; // 稳定帧不再扫描任何摆放记录。
    }
    let end = (state.index.cursor + INDEX_BUDGET_PER_FRAME).min(draft.placements.placements.len());
    for index in state.index.cursor..end {
        let record = &draft.placements.placements[index]; // 每帧只触碰固定数量的小型记录。
        state.index.by_placement_id.insert(record.id.clone(), index);
        if let Some(group) = &record.courtyard {
            state
                .index
                .by_instance_id
                .entry(group.instance_id.clone())
                .or_default()
                .push(index);
        }
    }
    state.index.cursor = end;
    state.index.ready = end == draft.placements.placements.len(); // 完整列表收集后才允许任何整组操作。
}
