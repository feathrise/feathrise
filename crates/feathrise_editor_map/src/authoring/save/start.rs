//! 保存请求的派生完成门、作者预览门与单一IO任务启动。

mod boundary_snapshot;
mod deletion_snapshot; // 删除ID与弱票数组由单飞CPU任务完整捕获，启动帧只共享O(1)源。
mod placement_water; // 整网格水记录分帧索引、根所有权筛选与完整Scene0证明保持独立纯职责。 // 空气墙点数组分帧捕获，避免保存启动帧全图复制。
mod water_snapshot; // 多个水画布的路径身份按8画布/64层预算冻结，不把整图元数据遍历放进启动帧。

use bevy::asset::AssetEvent; // 相关Scene0资产变化按固定消息预算唤醒冻结候选。
use bevy::ecs::message::{MessageCursor, Messages}; // 独立显式游标检测双缓冲淘汰，MessageReader无法报告丢失量。
use bevy::ecs::system::SystemParam; // 保存入口的多项只读正式证据收拢后保持在Bevy系统参数上限内。
use bevy::prelude::*; // 保存系统资源与后台任务句柄。
use bevy::tasks::IoTaskPool; // 完整作者快照只在有界IO任务中落盘。
use bevy::world_serialization::WorldAsset; // 保存门拥有独立WorldAsset消息读者，不读取或复制World内容。
use feathrise_map_content::{
    ActiveDynamicMap, BuildingPresetRegistry, MAP_MANIFEST_FILE_NAME, MapTerrainKind,
}; // 当前玩法地图身份、建筑目录、摆放作者快照与动态高度场水预览契约。
use feathrise_scene::{
    DynamicWaterMeshOverrideStatus, DynamicWaterMeshOverrides, DynamicWaterStatus,
    MapPlacementInstance, MapPlacementRuntimeStatus, MapPlacementWaterEvidence,
}; // 保存前只接受Scene0真实验证Ready的网格水，并冻结正式运行时代次。

use super::super::resources::{
    DraftSaveTask, EditorAuthoringSession, GlobalEnvironmentAuthoringState,
    GlobalEnvironmentSaveTask, PlacementPreviewHandoffSnapshot, PlacementSaveHandoffSnapshot, PlacementWaterEvidenceRegistry,
    TerrainLiveApplyState, TerrainPreviewState, WaterBrushStroke, WaterPreviewState,
    WaterSaveHandoffSnapshot,
}; // 保存single-flight、作者修订、活动水笔划与联合水预览证据。
use super::super::vegetation_compile::VegetationCompileState; // 保存只能克隆与作者修订一致的完整正式输出。
use super::super::water_handoff::WaterFileHandoff; // 旧水文件交接完成前不能被第二次保存覆盖。
use super::super::{PlacementGroundingReadiness, PlacementTerrainFollowState}; // 摆放快照必须等待当前地形修订的分帧重贴地完成。
use super::super::{SurfaceBrushStroke, SurfaceLayerEditTask}; // 地表删除和共享写侧待补段必须先形成完整作者版本。
use crate::components::EditorPlacedAsset; // 未保存预览必须由当前session精确实体映射认领。
use crate::resources::{MapDeletionTask, MapEditorState};
pub(crate) use placement_water::PlacementWaterSaveReadiness; // 保存入口与测试共用最终Ready/Waiting/Failed枚举。
#[cfg(test)]
pub(crate) use placement_water::placement_water_save_readiness; // 纯根聚合oracle只在回归测试中保留公开路径。
#[cfg(test)]
use placement_water::{
    PLACEMENT_WATER_RECORD_SCAN_BUDGET_PER_FRAME, PlacementWaterRootScope,
    placement_water_root_scope,
};
use placement_water::{
    advance_placement_water_evidence_cache, advance_placement_water_record_cache,
    consume_relevant_world_asset_events, placement_water_cached_readiness,
};
pub(crate) use placement_water::{
    observe_added_placement_water_status, observe_removed_placement_water_status,
    observe_replaced_placement_water_status, register_placement_water_evidence_root,
    unregister_placement_water_evidence_root,
}; // 插件注册生命周期observer，稳定帧不执行全量Changed或marker Query。 // 保存系统只编排两级有界游标与缓存完成值。 // 删除互斥、保存请求与关闭状态。 // 子模块回归直接验证固定预算。

const MAX_PLACEMENT_HANDOFF_PREVIEWS: usize = 8_192; // 与正式显式摆放全图硬预算相同，保存快照绝不无界增长。

/// 保存启动时读取的当前玩法地图及正式完成证据。
#[derive(SystemParam)]
pub(crate) struct SaveRuntimeReadiness<'w, 's> {
    placement_previews: Query<'w, 's, (Option<&'static MapPlacementInstance>, Option<&'static Transform>,
        Option<&'static ChildOf>, Option<&'static DynamicWaterMeshOverrides>), With<EditorPlacedAsset>>, // 仅沿session地址直接get，不扫描世界。
    vegetation_cross: Option<Res<'w, crate::authoring::vegetation_paint_scope::VegetationPaintState>>,
    terrain_objects: Option<Res<'w, crate::ui::terrain_objects::TerrainObjectsEditorState>>,
    terrain_content: Option<Res<'w, crate::authoring::content_bindings::ContentTerrainFollowState>>,
    volume: Option<Res<'w, crate::authoring::volume_sculpt::VolumeAuthoringState>>, // 尚未dirty的冻结点击和联合碰撞也必须先收口。
    folder_persistence: Option<Res<'w, crate::resources::MapFolderPersistence>>, // 名称事务不能与旧清单保存并发。
    boundary_capture: Local<'s, boundary_snapshot::BoundarySaveCapture>, // 复用系统参数组，不突破系统参数数量。
    water_capture: Local<'s, water_snapshot::WaterSaveCapture>,
    deletion_capture: Local<'s, deletion_snapshot::DeletionSaveCapture>,
    authoring_retirement: Option<ResMut<'w, crate::authoring::AuthoringRetirement>>, // 独立保存工具缺省时由捕获槽承担有界后台退役。
    boundary_edit: Option<Res<'w, crate::authoring::BoundaryEditState>>, // 实时拖点未收口时不冻结半笔。
    boundary_numeric: Option<Res<'w, crate::authoring::boundary::BoundaryThicknessInputState>>, // 三个墙体数值字段共享收口门。
    surface_task: Option<Res<'w, SurfaceLayerEditTask>>, // 可选资源保持不含地表UI的既有保存测试和独立工具兼容。
    puddles: Option<Res<'w, crate::authoring::PuddleAuthoringState>>, // 保存等待积水整笔后台收口。
    surface_stroke: Option<Res<'w, SurfaceBrushStroke>>, // 已受理但尚未dirty的共享等待单击同样不能被保存越过。
    active_map: Res<'w, ActiveDynamicMap>,               // 只有当前玩法地图保存需要建立正式交接门。
    dynamic_water: Res<'w, DynamicWaterStatus>,          // IO启动前冻结当前水域期望代次。
    root_water_draft: Option<Res<'w, feathrise_scene::DynamicClipmapWaterDraft>>,
    water_live: Option<Res<'w, super::super::live_water_dynamic::LiveClipmapWaterState>>,
    placements: Res<'w, MapPlacementRuntimeStatus>,      // 新摆放Ready必须严格越过当前修订。
    water_preview: Res<'w, WaterPreviewState>,           // 完整作者水网必须已由正式构建器预检。
    terrain_preview: Res<'w, TerrainPreviewState>, // 任意F12动态地图保存都要求当前修订全套派生完成。
    terrain_live: Res<'w, TerrainLiveApplyState>,  // 高度保存必须持有已原子采用的临时Mesh身份。
    placement_grounding: Res<'w, PlacementTerrainFollowState>, // 跟随型资产完整采用前不得冻结旧世界Y。
    building_registry: Option<Res<'w, BuildingPresetRegistry>>, // 建筑目录尚未Ready时保存请求继续等待。
    building_state: Option<Res<'w, crate::authoring::buildings::BuildingAuthoringState>>, // 当前联合身份BVH与审计是唯一建筑保存信用。
    courtyard_state: Option<Res<'w, crate::authoring::courtyards::CourtyardAuthoringState>>, // 整组分页校验尚未完成时保留保存请求。
    clipmap: Option<Res<'w, feathrise_scene::DynamicClipmapGroundSurface>>, // 最终保存门再次核对基础地形内容来源。
}

/// 保存按钮只克隆当前完整草稿并启动一个IO任务；保存期间编辑被暂时拒绝。
pub(crate) fn start_draft_save(
    domain_state: Option<Res<crate::game_authoring::map_interaction::MapInteractionState>>, // 保存不得截断外部作者的后台笔划。
    mut editor: ResMut<MapEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut save: ResMut<DraftSaveTask>,
    global_environment: Res<GlobalEnvironmentAuthoringState>, // 地图与全局同时脏时冻结到同一跨文件事务。
    global_save: Res<GlobalEnvironmentSaveTask>, // 单文件全局任务存在时禁止启动联合任务。
    compile: Res<VegetationCompileState>,
    deletion: Res<MapDeletionTask>, // 最后一道低层门防止迟到保存与catalog回收并发。
    mut readiness: SaveRuntimeReadiness, // 收拢只读正式证据及分帧捕获，避免越过Bevy顶层SystemParam数量上限。
    water_handoff: Res<WaterFileHandoff>, // 连续保存必须串行保留上一事务的R8/兼容高度源/目录失败恢复责任。
    water_stroke: Res<WaterBrushStroke>,  // pending快速单击必须先补画收口，保存请求保持到下一帧。
    asset_server: Res<AssetServer>, // 只读预览根已申请Scene0句柄的完整加载状态，不发起任何新IO。
    scene_events: Res<Messages<AssetEvent<WorldAsset>>>, // 只读标准双缓冲资产消息集合。
    mut scene_event_cursor: Local<MessageCursor<AssetEvent<WorldAsset>>>, // 跨帧固定预算游标保留积压起点并检测消息丢失。
    placement_water_registry: Res<PlacementWaterEvidenceRegistry>, // observer增量表替代每帧Query枚举。
    placement_water_roots: Query<
        (
            &MapPlacementInstance,      // 冻结MapId、资产路径、原始变换及三态批次所有权。
            &DynamicWaterMeshOverrides, // 当前可热改水数组补齐完整记录证明。
            Option<&DynamicWaterMeshOverrideStatus>, // Pending、Ready或具体Failed运行时回执。
            &WorldAssetRoot,            // 只读Scene0及依赖加载失败，不发起新IO。
            Option<&EditorPlacedAsset>, // 精确区分作者预览与正式/候选根。
        ),
        With<MapPlacementWaterEvidence>,
    >, // 只按observer登记的Entity直接get；8192个普通placement不进入等待帧查询。
) {
    readiness.deletion_capture.poll(&session, editor.save_requested, readiness.authoring_retirement.as_deref_mut());
    if session.water_preset_delete_pending { return; } // 引用删除收口前保留保存意图，不能与全局目录事务交叉发布。
    // 最先轮询：请求撤销、换图或其他领域仍在等待时，也不能遗弃持有旧源的后台任务。
    if readiness.terrain_objects.as_ref().is_some_and(|state| state.busy()) { return; } // 对象完整采用前不冻结过期集合。
    if readiness.vegetation_cross.as_ref().is_some_and(|state| state.busy()) {return;} // 本笔与全部成员图片收口前保留保存意图。
    let scene_event_progress = consume_relevant_world_asset_events(
        &scene_events,
        &mut scene_event_cursor,
        &mut save.placement_water_preflight,
        placement_water_registry.status_revision, // 之后同根状态序号必须严格越过此资产事件基线才能恢复Ready信用。
        editor.save_requested,
    ); // 即使没有保存请求也推进独立消息游标，未来请求不会重放历史GLB事件。
    if readiness
        .folder_persistence
        .as_deref()
        .is_some_and(|state| state.rename_task.is_some())
    {
        return; // 保留保存请求，等名称采用到草稿后再冻结快照。
    }
    if !editor.save_requested {
        *readiness.boundary_capture = default(); // 已取消的请求不保留待保存墙点。
        *readiness.water_capture = default();
        return;
    }
    if let Some(content) = readiness.terrain_content.as_ref() {
        if let Some(error) = &content.failure {
            editor.save_requested = false; // 确定失败停止自动重试，保存与关闭不能继续越过关联完整性门。
            editor.cancel_close_with_error(format!(
                "地形关联内容尚未形成完整版本，保存未开始：{error}；本次关闭已取消"
            )); // 复用明确错误回执，让统一底栏保留故障，同时保留当前多地形草稿。
            return;
        }
        if !content.ready(&session) { editor.status = "等待地形关联内容完整更新后保存…".into(); return; }
    }
    if readiness.volume.as_ref().is_some_and(|state| state.busy()) {
        editor.status = "等待三维雕刻、洞口和真实碰撞完整收口后保存…".into();
        return; // 保留请求，不能保存中间密度或尚未生效的洞口。
    }
    if readiness
        .boundary_edit
        .as_ref()
        .is_some_and(|edit| edit.dragging())
        || readiness
            .boundary_numeric
            .as_ref()
            .is_some_and(|input| input.busy())
    {
        editor.status = "空气墙编辑尚未完成；松手或完成输入后继续保存".into(); // 捕获不能读取尚未推进修订的中间值。
        return;
    }
    if domain_state.as_ref().is_some_and(|state| state.busy()) {
        editor.status = "外部作者操作正在后台完成；随后自动保存整张地图".into();
        return;
    } // 保留请求不截断一次笔划。
    if readiness // 依据完整作者条件选择有限处理分支。
        .surface_task // 完成当前地表作者事务的受检参数或值转换。
        .as_deref() // 完成当前地表作者事务的受检参数或值转换。
        .is_some_and(SurfaceLayerEditTask::is_deleting) // 地表删除和写侧准备共用single-flight任务槽。
        || readiness // 依据完整作者条件选择有限处理分支。
            .surface_stroke // 完成当前地表作者事务的受检参数或值转换。
            .as_deref() // 完成当前地表作者事务的受检参数或值转换。
            .is_some_and(|stroke| stroke.active)
    // 只继续处理存在且身份仍匹配的作者值。
    {
        // 满足上述完整条件后才进入当前分支。
        // 删除的定义/像素和最后一笔必须先同修订原子收口。
        editor.status = "当前地表作者操作正在后台完成；收口后会自动继续保存".to_owned(); // 当前完整地表继续可见，保存请求稍后自动继续。
        return; // 不消费save_requested，也不冻结旧草稿或启动IO。
    } // 地表后台作者事务保存门结束。
    let scene_events_caught_up = match scene_event_progress {
        Ok(caught_up) => caught_up, // 没有消息淘汰时继续使用固定预算追平结果。
        Err(missed) => {
            editor.save_requested = false; // 事件身份已经不可恢复，本次保存必须零IO失败关闭。
            editor.cancel_close_with_error(format!(
                "地图保存已停止：Scene0资产事件读者因积压丢失{missed}条消息，无法证明整网格水属于最新GLB；请重新保存"
            ));
            return; // 显式游标已经追到当前尾部，下一次人工保存会从完整新状态重新预检。
        }
    };
    if deletion.busy() {
        editor.save_requested = false; // 已确认删除期间不保留可迟到的写盘意图。
        editor.status = "地图回收事务完成前不能保存".to_owned();
        return;
    }
    if readiness.puddles.as_ref().is_some_and(|state| state.busy()) {
        // 等待整笔后台块与历史共同完成。
        editor.status = "等待积水笔划完成后保存…".into(); // 不消费保存请求。
        return; // 不能保存半个画布版本。
    } // 结束积水保存门。
    if water_stroke.active {
        editor.status = "当前水笔刷段正在等待作者源并收口；完成后会自动继续保存".to_owned();
        return; // 不消费save_requested，防止未dirty的pending点击被保存门越过。
    }
    if water_handoff.blocks_new_save_for(session.draft.as_ref().map(|draft| &draft.map_id)) {
        editor.status =
            "上一笔保存或回滚仍在等待正式水域/摆放完整交接；完成后会自动继续本次保存".to_owned();
        return; // 保留save_requested；未解决态只允许同图完整保存接管，其他门必须先收敛。
    }
    if !scene_events_caught_up {
        editor.status = "正在分帧追平Scene0资产变化，完成后继续整网格水保存验证".to_owned();
        return; // 未读事件可能属于当前候选，追平前不能用旧Ready信用启动任何IO。
    }
    if readiness
        .courtyard_state
        .as_deref()
        .is_some_and(|state| state.save_pending())
    {
        editor.status = "院落模板、成员成组或地基校验尚未完成，完成后继续地图保存".into();
        return;
    }
    // 建筑域统一核对当前索引与地基来源；空地图不能先被旧空索引的来源票据挡住。
    match crate::authoring::buildings::save_readiness(
        &session,
        readiness.building_registry.as_deref(),
        readiness.building_state.as_deref(),
        readiness.clipmap.as_deref(),
    ) {
        crate::authoring::buildings::BuildingSaveReadiness::Ready => {}
        crate::authoring::buildings::BuildingSaveReadiness::Waiting(detail) => {
            editor.status = format!("正在等待建筑保存预检：{detail}；完成后自动继续保存");
            return; // 保留save_requested，索引和审计系统下一帧继续推进。
        }
        crate::authoring::buildings::BuildingSaveReadiness::Failed(detail) => {
            editor.save_requested = false; // 确定非法身份不在稳定帧重复尝试。
            editor.cancel_close_with_error(format!(
                "地图保存已停止：建筑预检失败：{detail}；请移动、旋转或删除对应建筑后重试"
            ));
            return; // 尚未克隆草稿或启动任何IO。
        }
    }
    if let Some(draft) = session.draft.as_ref() {
        match readiness.placement_grounding.readiness_for(
            &draft.map_id,                        // 当前待保存地图身份。
            session.terrain_revision,             // 当前地形高度作者修订。
            session.placement_structure_revision, // 当前摆放数组下标语义。
            draft.placements.placements.len(),    // 完整文档记录总数。
        ) {
            PlacementGroundingReadiness::Ready => {} // 全部跟随记录已经写回当前解析世界Y。
            PlacementGroundingReadiness::Waiting { processed, total } => {
                editor.status = format!(
                    "正在分帧让跟随型资产匹配当前地形：{processed}/{total}；完成后自动继续保存"
                );
                return; // 保留save_requested，下一帧从既有游标继续。
            }
            PlacementGroundingReadiness::Failed(detail) => {
                editor.save_requested = false; // 确定坏高度场或记录不在稳定帧重复保存。
                editor.cancel_close_with_error(format!(
                    "地图保存已停止：跟随型资产重贴地失败：{detail}；请修正高度模式或地形后重试"
                ));
                return; // 首个文件stage前失败，磁盘与上一正式摆放保持不变。
            }
        }
        let scan = match advance_placement_water_record_cache(
            &mut save.placement_water_preflight,
            session.revision,
            session.placement_structure_revision,
            &draft.placements,
        ) {
            Ok(scan) => scan, // 当前帧最多读取固定256条placement记录。
            Err(detail) => {
                editor.save_requested = false; // 公共绑定预算确定失败后停止稳定帧自动重试。
                editor.cancel_close_with_error(format!("地图保存已停止：{detail}"));
                return; // 非空记录索引完成前不查询世界、不启动任何IO。
            }
        };
        if !scan.complete {
            editor.status = format!(
                "正在分帧检查整网格水体作者记录：{}/{}（本帧{}项）",
                scan.scanned, scan.total, scan.processed,
            );
            return; // 保留save_requested；下一帧从固定游标继续，不重复扫描已完成前缀。
        }
        let evidence = advance_placement_water_evidence_cache(
            &mut save.placement_water_preflight,
            &placement_water_registry,
            &readiness.placements,
            &draft.map_id,
            &placement_water_roots,
            &asset_server,
            &session.placement_previews,
        ); // 首次只读取64个登记实体；稳定Waiting只轮询至多32个冻结候选。
        if evidence.collecting {
            editor.status = format!(
                "正在分帧收集整网格水体Scene0证据：{}/{}（本帧{}根）",
                evidence.roots_scanned, evidence.roots_total, evidence.roots_processed,
            );
            return; // 完整成员表枚举前不把尚未读到的后段唯一Ready根误判为缺失。
        }
        match placement_water_cached_readiness(&save.placement_water_preflight) {
            PlacementWaterSaveReadiness::Ready => {}
            PlacementWaterSaveReadiness::Waiting(detail) => {
                editor.status = format!("正在等待整网格水体保存前验证：{detail}");
                return; // 保留save_requested，运行时有界队列Ready后下一帧自动继续保存。
            }
            PlacementWaterSaveReadiness::Failed(detail) => {
                editor.save_requested = false; // 确定失败不在稳定帧重复覆盖同一可见诊断。
                editor.cancel_close_with_error(format!(
                    "地图保存已停止：{detail}；请修正Blender节点、UV、材质或水体绑定后重试"
                ));
                return; // 任何文件stage前失败，旧正式摆放与磁盘保持完整不变。
            }
        }
    }
    let vegetation_needs_compile = session
        .draft
        .as_ref()
        .is_some_and(|draft| {
            // 容器的空植被描述没有作者画布，编译器不会受理；不能等待不存在的任务。
            // 旧单地形与已活动网格仍须同修订完成；载入中也不能借active暂空越过保存门。
            draft.vegetation.is_some()
                && (draft.manifest.terrains.is_none()
                    || draft.terrain_documents.active.is_some()
                    || draft.terrain_documents.loading)
        })
        && session.vegetation_compiled_revision != session.vegetation_revision; // 保存不能发布新作者图配旧正式密度。
    if vegetation_needs_compile {
        let failed = session.draft.as_ref().is_some_and(|draft| {
            compile.failed_map_id.as_ref() == Some(&draft.map_id)
                && compile.failed_revision == Some(session.vegetation_revision)
        }); // 同修订确定失败时停止自动等待，保留草稿供作者修复。
        if failed {
            editor.save_requested = false; // 不在稳定帧重复覆盖编译失败诊断。
            editor.cancel_close_with_error(
                "地图保存已停止：当前植被作者图层编译失败，本次关闭已取消；请修正图层或组参数后重试",
            ); // 正式输出仍保持上一完整版本；确定失败必须归还F12笔刷输入。
        } else {
            editor.status = "正在等待植被作者图层后台编译完成，随后自动继续原子保存".to_owned(); // 请求保持为true。
        }
        return;
    }
    let terrain_preview_requirement = session.draft.as_ref().and_then(|draft| {
        (draft.terrain_dirty
            && draft.manifest.is_data_driven()
            && draft.manifest.legacy_terrain().kind == MapTerrainKind::Heightfield)
            .then_some((&draft.map_id, session.terrain_revision))
    }); // 当前地图是否正在运行不影响terrain.glb落盘前的完整派生证明。
    if let Some((map_id, terrain_revision)) = terrain_preview_requirement {
        if readiness.terrain_live.failed_for(map_id, terrain_revision) {
            editor.save_requested = false; // 确定失败时归还保存按钮，避免旧请求稳定帧循环。
            editor.cancel_close_with_error(
                "地图保存已停止：当前GLB地形无法生成完整内存高度、法线或Trimesh；请修正地形后重试",
            );
            return;
        }
        if !readiness
            .terrain_preview
            .ready_for(map_id, terrain_revision)
        {
            editor.status =
                "当前GLB地形修订仍在后台生成高度、法线和Trimesh，完成后自动继续保存".to_owned();
            return; // 自动保存/关闭请求可以等待；人工按钮在UI入口已直接禁用。
        }
    }
    let water_preview_requirement = session.draft.as_ref().and_then(|draft| {
        (draft.terrain_manifest().is_data_driven()
            && (matches!(draft.terrain_manifest().terrain.kind, MapTerrainKind::Heightfield | MapTerrainKind::SparseClipmap)
                || draft.terrain_documents.active.is_none() && draft.manifest.terrain.is_empty())
            && draft.water.as_ref().is_some_and(|water| {
                !water.layers.is_empty() || !water.rivers.is_empty() || !water.waterfalls.is_empty()
            }))
        .then(|| {
            (
                &draft.map_id,
                session.water_revision,
                session.terrain_revision,
            )
        })
    }); // 与关闭门使用相同联合身份；静水构建失败也必须发生在首次文件写入前。
    if let Some((map_id, water_revision, terrain_revision)) = water_preview_requirement {
        if readiness
            .water_preview
            .failed_for(map_id, water_revision, terrain_revision)
        {
            editor.save_requested = false; // 同修订已经确定失败，停止稳定帧重复重试并归还作者输入。
            editor.cancel_close_with_error(
                "地图保存已停止：当前静水、河流或瀑布正式预览构建失败，本次关闭已取消；请修正水域后重试",
            );
            return;
        }
        if !readiness
            .water_preview
            .ready_for(map_id, water_revision, terrain_revision)
        {
            editor.status = readiness.water_preview
                .waiting_detail_for(map_id, water_revision, terrain_revision)
                .map(|detail| format!("{detail}；完成后自动继续保存"))
                .unwrap_or_else(|| "正在等待当前水域完成同修订后台预检，随后自动继续保存".to_owned());
            // 保留save_requested；只报告当前版本的真实进度，不能再把Clipmap预检笼统归为GLB构建。
            return;
        }
        // 已采用的作者预览证明静水和已完成样条可由正式共享构建器生成；下方保存预检再严格拒绝中间态。
    }
    if let Some(draft) = session.draft.as_ref().filter(|draft| draft.terrain_documents.active.is_some()) {
        let root = super::super::terrain_documents::root_water::source(draft);
        if root.water.is_some_and(|water| !water.layers.is_empty() || !water.rivers.is_empty() || !water.waterfalls.is_empty())
            && readiness.active_map.map_id.as_ref() == Some(&draft.map_id) {
            let expected = super::super::terrain_documents::root_water::revision(draft, session.water_revision);
            let status = &readiness.dynamic_water;
            if let Some(error) = readiness.water_live.as_deref().and_then(|live| live.root_failure_for(&draft.map_id, expected)) {
                editor.save_requested = false;
                editor.cancel_close_with_error(format!("地图保存已停止：根静水后台准备失败：{error}"));
                return;
            }
            let published = readiness.root_water_draft.as_deref();
            let source_matches = published.and_then(|published| published.root_for_map(&draft.map_id))
                .is_some_and(|snapshot| snapshot.source_revision == expected);
            let published_revision = published.and_then(|published| published.root_revision_for_map(&draft.map_id));
            if !source_matches || status.map_id.as_ref() != Some(&draft.map_id) || status.input_draft_revision != published_revision
                || status.phase != feathrise_scene::DynamicWaterPhase::Ready
                || status.applied_generation != status.desired_generation {
                if source_matches && status.map_id.as_ref() == Some(&draft.map_id) && status.input_draft_revision == published_revision
                    && status.phase == feathrise_scene::DynamicWaterPhase::Failed {
                    editor.save_requested = false;
                    editor.cancel_close_with_error(format!("地图保存已停止：根静水正式预览失败：{}", status.detail));
                } else { editor.status = "正在等待地图根静水的同修订正式水面与GPU遮罩完整采用，随后自动继续保存".into(); }
                return; // 当前private预览Ready不能替代停放根水自己的正式CPU/GPU证据。
            }
        }
    }
    let terrain_live_requirement = session.draft.as_ref().and_then(|draft| {
        (draft.terrain_dirty
            && draft.manifest.is_data_driven()
            && draft.manifest.legacy_terrain().kind == MapTerrainKind::Heightfield
            && readiness.active_map.map_id.as_ref() == Some(&draft.map_id))
        .then_some((&draft.map_id, session.terrain_revision))
    }); // 当前玩法高度保存必须从已验证的内存联合候选开始，非当前地图只更新磁盘文件。
    if let Some((map_id, terrain_revision)) = terrain_live_requirement {
        if readiness.terrain_live.failed_for(map_id, terrain_revision) {
            editor.save_requested = false; // 确定失败不在稳定帧无限覆盖同一诊断或锁住作者输入。
            editor.cancel_close_with_error(
                "地图保存已停止：当前未保存地形的实时Mesh/Trimesh派生失败；请继续修改地形后重试",
            );
            return;
        }
        if !readiness.terrain_live.ready_for(map_id, terrain_revision) {
            editor.status =
                "正在等待未保存地形画面与Trimesh碰撞原子应用，完成后自动继续保存".to_owned();
            return; // 保留save_requested；不能在临时Mesh身份尚未稳定时启动无法证明的新文件交接。
        }
    }
    if let Some(draft) = session.draft.as_ref() {
        let identity = Some((draft.map_id.clone(), session.revision)); // 同一作者完整版本才能提交。
        if readiness.boundary_capture.identity != identity {
            *readiness.boundary_capture = boundary_snapshot::BoundarySaveCapture {
                identity,
                capture: default(),
            }; // 修改或换图时丢弃旧捕获。
        }
        if let Some(boundaries) = draft.boundaries.as_ref() {
            if !readiness
                .boundary_capture
                .capture
                .step(boundaries, 4096, 64)
            {
                editor.status = "正在准备空气墙保存快照…".into(); // 保留请求，继续响应编辑器输入。
                return;
            }
        }
    }
    if !editor.dirty {
        editor.save_requested = false;
        editor.status = "当前地图没有未保存变更".to_owned();
        return; // 已通过实时采用预检的干净文档没有IO批次，不再捕获水文件交接表。
    }
    if !readiness.water_capture.step(&session, &water_handoff) {
        editor.status = "正在准备独立水画布的保存交接身份…".into();
        return;
    }
    if !readiness.deletion_capture.step(&session) {
        editor.status = "正在后台准备摆放删除的保存确认快照…".into();
        return; // 完整删除scope与独立修订匹配前保留请求，ack换代也不能借session.revision旧值通过。
    }
    editor.save_requested = false; // 所有派生输入和空气墙、水画布、删除身份快照完整后消费请求。
    if save.task.is_some() || global_save.task.is_some() || session.saving {
        editor.status = "地图保存任务正在执行".to_owned();
        return;
    }
    if session.placement_previews.len() > MAX_PLACEMENT_HANDOFF_PREVIEWS {
        editor.cancel_close_with_error(format!(
            "地图保存已停止：草稿Scene0预览超过{}项的交接硬预算",
            MAX_PLACEMENT_HANDOFF_PREVIEWS
        ));
        return; // 不启动IO，也不截断实体快照；异常文档必须先由作者修正。
    }
    let Some(source) = session.draft.as_ref() else {
        editor
            .cancel_close_with_error("地图作者数据尚未就绪，本次关闭已取消；请等待地图加载完成后重试"); // 保存没有可冻结的权威草稿时不能继续消费关闭请求。
        return;
    };
    let boundaries = if source.boundaries.is_some() {
        let Some(completed) = readiness.boundary_capture.capture.take() else {
            editor.save_requested = true; // 不允许丢失墙对象后继续保存其他领域。
            return;
        };
        Some(completed) // 直接移交已完成墙点，不再次clone。
    } else {
        None
    }; // 没有文档时不采用上一请求残留值。
    let deleted = readiness.deletion_capture.take(&session)
        .expect("保存请求消费前已确认完整删除快照，当前系统内作者删除表不会被其他系统修改");
    let draft = boundary_snapshot::snapshot(source, boundaries); // 其他领域维持既有共享快照，不临时修改源草稿。
    let global_environment_snapshot = global_environment
        .dirty
        .then(|| global_environment.draft.clone()); // 只有未保存全局草稿才加入地图批次，地图单独保存不制造无关全局写入。
    save.global_environment_snapshot = global_environment_snapshot
        .as_ref()
        .map(|asset| (global_environment.revision, asset.clone())); // 主线程成功采用必须复核同一全局修订。
    save.water_handoff_snapshot = (readiness.active_map.map_id.as_ref() == Some(&draft.map_id))
        .then(|| {
            let dirty_layers = readiness.water_capture.take_root();
            let terrains = readiness.water_capture.take();
            let root = super::super::terrain_documents::root_water::source(&draft);
            if root.water.is_none() && terrains.is_empty() { return None; }
            Some(WaterSaveHandoffSnapshot {
                map_id: draft.map_id.clone(),
                water_revision: session.water_revision,
                root_water_revision: super::super::terrain_documents::root_water::revision(&draft, session.water_revision),
                terrain_revision: session.terrain_revision,
                baseline_generation: readiness.dynamic_water.desired_generation,
                manifest_asset_path: format!("{}/{}", draft.folder.trim_end_matches('/'), MAP_MANIFEST_FILE_NAME),
                water_asset_path: root.water.map_or_else(String::new, |_| root.asset_path.to_owned()),
                dirty_layers,
                dirty_height_path: (draft.manifest.terrains.is_none() && draft.terrain_dirty)
                    .then(|| draft.manifest.legacy_terrain().height_path.clone()).flatten(),
                catalog_registration_required: root.registration_required,
                terrains,
            })
        }).flatten(); // 根水与全部私有水同批冻结，文件和正式Ready按各自来源串行交接。
    save.placement_handoff_snapshot = Some(PlacementSaveHandoffSnapshot {
        map_id: draft.map_id.clone(), // 非当前地图不建正式门，但仍用同一精确快照分帧回收本批预览。
        baseline_revision: readiness.placements.revision, // 新正式根必须严格越过本次IO启动时已采用值。
        baseline_attempt_revision: readiness.placements.attempt_revision, // 保存前旧Failed不能冒充本次发布失败。
        retry_failed_runtime: readiness.active_map.map_id.as_ref() == Some(&draft.map_id)
            && water_handoff.placement_retry_required_for(&draft.map_id), // 只有当前玩法地图允许原样重建正式候选。
        editor_previews: session
            .placement_previews
            .iter()
            .filter_map(|(id, entity)| readiness.placement_previews.get(*entity).ok().and_then(|(instance, transform, parent, water)|
                PlacementPreviewHandoffSnapshot::from_components(&draft.map_id, id, *entity, instance, transform, parent, water)))
            .collect(), // 沿既有8192地址上限冻结O(1)版本/姿态证据，同Entity上的后来作者值也不属于本批。
        deleted, // 移动后台完成且scope/独立删除修订仍匹配的弱票；完成帧不得直接清空当前删除表。
    }); // 即使没有水域文件，整网格水和普通摆放仍共享精确预览生命周期。
    let revision = session.revision;
    save.ticket = save.ticket.wrapping_add(1);
    let ticket = save.ticket;
    session.saving = true;
    session.loaded_migration_revision = None; // 文件事务开始后必须保留成功/失败及正式交接责任，不能按未编辑加载草稿离开。
    let volume_files = readiness.volume.as_ref().map_or_else(Vec::new, |state| state.save_files());
    save.task = Some(IoTaskPool::get().spawn(async move {
        (
            ticket,
            revision,
            super::batch::save_draft_with_volumes(&draft, global_environment_snapshot.as_ref(), &volume_files).await, // 自动GLB与placements同批发布，失败共同回滚。
        )
    }));
    // 可选Blender是否可用由后台检查；启动反馈不提前宣称会生成Blend。
    editor.status = if global_environment.dirty {
        "正在后台原子联合保存全局环境、地图清单、JSON和作者资源".to_owned()
    } else {
        "正在后台原子保存地图清单、JSON、作者资源和地形GLB".to_owned() // 无动态地形变化时不无意义启动Blender。
    };
}

#[cfg(test)]
mod tests; // 大规模记录缓存与根身份回归独立文件，保持保存系统入口短小。
#[cfg(test)]
mod vegetation_tests; // 空容器与真实网格分别验证植被编译保存门，避免永久等待不存在的任务。
