//! 当前地图建筑 placement 的固定预算收集与后台 BVH 原子采用。

use std::sync::Arc; // 采用完成后只共享不可变索引，不复制最多8192项几何。

use bevy::prelude::*; // ECS 资源系统参数。
use bevy::tasks::AsyncComputeTaskPool; // 纯 CPU BVH 构建进入计算池。
use bevy::tasks::futures::check_ready; // 主线程只非阻塞轮询后台任务。
use feathrise_map_content::{ActiveDynamicMap, BuildingPresetRegistry, BuildingWorldQuad, MapId}; // 当前地图身份、建筑目录与植被净空几何。
use feathrise_scene::{
    BuildingVegetationClearanceAuthoringIdentity, BuildingVegetationClearanceState,
    DynamicClipmapGroundSurface,
}; // 已完成BVH向正式植被与F12预览发布同一未保存草稿净空。
use feathrise_scene_core::state::ScenePhase; // 关闭F12后只为仍在玩法中的同一地图保留草稿净空。

use super::resources::BuildingAuthoringState; // 唯一收集、任务和采用槽。
use super::types::*; // 联合身份、派生项和构建结果。
use crate::authoring::{EditorAuthoringSession, PlacementTerrainSampler, TerrainPreviewState}; // 当前地图草稿、作者修订与共享高度采样。
use crate::resources::MapEditorState; // F12 生命周期和全局状态栏。
use crate::ui::building_presets::BuildingPresetEditorState; // 左栏回显构建失败原因。
use crate::ui::courtyard::{CourtyardUiState, CourtyardViewportMode}; // 院落盖印等待同一建筑索引时也需要就绪反馈。

const GATHER_RECORD_BUDGET_PER_FRAME: usize = 128; // 与正式 placement 规划预算一致，避免单帧扫描8192项。

#[cfg(test)]
#[path = "index_save_tests.rs"]
mod save_tests; // 真实多网格索引等待、恢复与保存三态回归独立维护。

/// 未保存摆放预览会留在玩法场景；其植被净空必须与可见建筑保持相同生命周期。
fn authoring_index_needed(
    editor: &MapEditorState,
    session: &EditorAuthoringSession,
    scene: Option<&State<ScenePhase>>,
    active_dynamic: Option<&ActiveDynamicMap>,
) -> bool {
    if editor.open {
        return true; // F12仍可编辑其他地图，继续为所选草稿建立完整索引。
    }
    if !editor.dirty && session.placement_previews.is_empty() {
        return false; // 保存交接已完成且没有草稿根时，正式placement重新拥有净空。
    }
    let Some(draft) = session.draft.as_ref() else {
        return false;
    };
    let runtime_map = match scene.map(State::get) {
        Some(ScenePhase::Active) => active_dynamic
            .and_then(|active| active.map_id.as_ref())
            .map(MapId::as_str),
        Some(ScenePhase::Idle | ScenePhase::Loading) | None => None,
    };
    runtime_map == Some(draft.map_id.as_str()) // 切图后的旧草稿不得继续覆盖正式植被。
}

/// 返回当前草稿与建筑目录的完整空间索引身份。
pub(super) fn current_identity(
    session: &EditorAuthoringSession,
    registry: &BuildingPresetRegistry,
) -> Option<BuildingIndexIdentity> {
    let draft = session.draft.as_ref()?; // 尚未加载地图时没有可构建空间。
    if registry.revision == 0 {
        return None; // 建筑目录尚无第一份完整有效快照。
    }
    Some(BuildingIndexIdentity {
        map_id: draft.map_id.clone(),               // 索引只属于当前地图。
        author_revision: session.revision,          // 数值移动、撤销与任何作者变更都拒绝旧几何。
        terrain_revision: session.terrain_revision, // 高度派生与联合整平结果必须属于当前地形修订。
        structure_revision: session.placement_structure_revision, // 数组结构变化立即重收集。
        catalog_revision: registry.revision,        // 占地或门口热更立即重建。
    })
}

/// 非阻塞接收一个 BVH 构建结果，并仅为当前期望身份原子采用。
pub(super) fn poll_index_build(
    mut state: ResMut<BuildingAuthoringState>,
    mut editor: ResMut<MapEditorState>,
    mut building_ui: ResMut<BuildingPresetEditorState>,
    mut courtyard_ui: ResMut<CourtyardUiState>, // 院落工具使用 Inspect，不能丢失索引就绪或失败提示。
) {
    let Some(task) = state.index_task.as_mut() else {
        return; // 稳定帧没有任务时保持 O(1)。
    };
    let Some(completed) = check_ready(task) else {
        return; // 未完成立即归还主线程。
    };
    state.index_task = None; // 完成值只消费一次并释放 single-flight 槽。
    if completed.discarded {
        return;
    }
    if state.desired_identity.as_ref() != Some(&completed.identity) {
        if completed.result.is_ok() {
            let identity = completed.identity.clone();
            state.index_task = Some(AsyncComputeTaskPool::get().spawn(async move {
                drop(completed); // 迟到完整索引和分页点集由原worker槽释放，不在主线程逐项析构。
                BuildingIndexBuildResult {
                    identity,
                    result: Err("旧建筑索引已回收".into()),
                    discarded: true,
                }
            }));
        }
        return; // 切图或修订变化后的迟到 BVH 无任何副作用。
    }
    let courtyard_uses_index = matches!(
        courtyard_ui.mode,
        CourtyardViewportMode::Stamp
            | CourtyardViewportMode::MoveGroup
            | CourtyardViewportMode::CopyGroup
    ); // 框选无需建筑索引，不能被索引状态覆盖提示。
    match completed.result {
        Ok(snapshot) => {
            let count = snapshot.index.len(); // 移入 Arc 前取得轻量统计。
            state.active_identity = Some(completed.identity.clone());
            state.active_index = Some(snapshot.index); // 完整 BVH 一次性替换活动值，缺页续接复用同一Arc。
            state.audit_ground = snapshot.ground_query;
            state.ground_content = snapshot.ground_content;
            state.audit_error = snapshot
                .audit_error
                .map(|error| (completed.identity.clone(), error)); // 非法旧位置仍保留索引供移动或删除修复。
            state.index_error = None; // 新有效版本清除同身份旧失败。
            let message = if state.audit_ground.is_some() {
                format!("建筑空间索引已就绪 · {count}栋；保存预检正在等待精确高度页")
            } else {
                state.audit_error.as_ref().map_or_else(
                    || format!("建筑空间索引与保存预检已就绪 · {count}栋现有建筑"),
                    |(_, error)| format!("建筑空间索引已就绪，但保存预检失败：{error}"),
                )
            };
            building_ui.report_status(message.clone());
            if editor.tool == crate::types::EditorTool::Building || courtyard_uses_index {
                editor.status = message.clone(); // 单栋与院落工具都依赖这一份完整索引。
            }
            if courtyard_uses_index
                && courtyard_ui
                    .status
                    .starts_with("院落操作未开始：建筑空间索引")
            {
                courtyard_ui.status = format!("{message}；请再次点击中央视口继续院落操作"); // 早到点击被拒后明确通知作者重试。
            }
        }
        Err(error) => {
            state.audit_error = None; // 索引本身失败时没有可授信的审计结果。
            state.ground_content = None; // 失败索引不能继续携带上一完整地形的保存信用。
            state.index_error = Some((completed.identity.clone(), error.clone()));
            let message = format!("建筑空间索引构建失败：{error}");
            building_ui.report_status(message.clone());
            if courtyard_uses_index {
                courtyard_ui.status = message.clone(); // 院落页直接显示确定失败原因，不让等待提示永久残留。
            }
            editor.status = message; // 无可靠索引时新建筑必须失败关闭。
        }
    }
}

/// 地形派生已就绪后，恢复曾被暂停的同身份索引收集，同时避免稳定帧重复构建。
fn needs_new_gather(state: &BuildingAuthoringState, current: &BuildingIndexIdentity) -> bool {
    state.desired_identity.as_ref() != Some(current)
        || (state.gather.is_none()
            && state.index_task.is_none()
            && (state.active_identity.as_ref() != Some(current) || state.active_index.is_none())
            && !state
                .index_error
                .as_ref()
                .is_some_and(|(failed, _)| failed == current)) // 同代确定失败等待真实输入修订。
}

/// 把已经完整采用的作者建筑索引发布为当前地图植被净空覆盖。
///
/// 【功能设计】收集前缀和在途 BVH 都不能影响画面；只有联合身份完整匹配的活动索引
/// 才一次复制固定大小四边形。未保存建筑留在玩法画面时保留其净空；正式交接、切图
/// 或索引失效后回退到正式 placement 快照。
pub(super) fn sync_vegetation_clearance_override(
    editor: Res<MapEditorState>,
    session: Res<EditorAuthoringSession>,
    scene: Option<Res<State<ScenePhase>>>,
    active_dynamic: Option<Res<ActiveDynamicMap>>,
    registry: Option<Res<BuildingPresetRegistry>>,
    state: Res<BuildingAuthoringState>,
    mut clearances: ResMut<BuildingVegetationClearanceState>,
) {
    let desired = authoring_index_needed(
        &editor,
        &session,
        scene.as_deref(),
        active_dynamic.as_deref(),
    )
    .then(|| registry.as_deref())
    .flatten()
    .and_then(|registry| current_identity(&session, registry)); // 没有可见草稿或任一完整输入缺失时不保留作者覆盖。
    let Some(identity) = desired else {
        clearances.clear_authoring(); // 自动恢复最近一次同图正式净空。
        return;
    };
    if state.active_identity.as_ref() != Some(&identity) {
        clearances.clear_authoring(); // 新身份仍在收集或后台构建时不发布旧作者索引。
        return;
    }
    let Some(index) = state.active_index.as_deref() else {
        clearances.clear_authoring(); // 防御身份与快照槽异常分叉。
        return;
    };
    let (regions, floor_heights) = index.vegetation_clearances();
    clearances.set_authoring_layered(
        BuildingVegetationClearanceAuthoringIdentity {
            map_id: identity.map_id.clone(), // 作者覆盖仅服务当前草稿地图。
            author_revision: identity.author_revision, // 移动、撤销和数值变换均使来源换代。
            structure_revision: identity.structure_revision, // 增删 placement 使数组来源换代。
            catalog_revision: identity.catalog_revision, // 预设净空热更使几何来源换代。
        },
        regions, // 后台冻结的四边形与高度只浅克隆Arc。
        floor_heights,
    );
}

#[cfg(test)]
mod tests {
    use super::*; // 用真实索引发布系统覆盖F12关闭后的草稿到正式净空交接。
    use feathrise_map_content::{
        BuildingFootprintRect, BuildingFoundationDescriptor, MapPlacementHeightMode,
        MapPlacementRecord, ResolvedBuildingPresetCatalog,
    };

    #[test]
    fn terrain_ready_resumes_building_index_gather_for_the_same_identity() {
        let mut draft =
            crate::authoring::terrain::tests::test_heightfield_draft(Arc::new(vec![0; 121]));
        let map_id = draft.map_id.clone();
        let ordinary = MapPlacementRecord {
            mesh_lod: None,
            terrain_binding: None,
            editor: Default::default(),
            terrain_patch: None,
            id: "ordinary-asset".to_owned(),
            asset_path: "maps/EDGE-TEST/ordinary.glb".to_owned(),
            asset_target: Default::default(),
            building_preset_id: None,
            courtyard: None,
            translation: [5.0, 0.0, 5.0],
            height_mode: MapPlacementHeightMode::FixedHeight,
            terrain_offset: 0.0,
            yaw_degrees: 0.0,
            scale: [1.0; 3],
            water_surfaces: Vec::new(),
        }; // 普通资产仍占扫描预算，但无需构造建筑预设。
        draft.placements.placements = (0..257)
            .map(|index| MapPlacementRecord {
                mesh_lod: None,
                terrain_binding: None,
                terrain_patch: None,
                id: format!("ordinary-{index}"),
                ..ordinary.clone()
            })
            .collect(); // 两帧后仍有一项，便于确认同一收集器持续推进。
        let session = EditorAuthoringSession {
            draft: Some(draft),
            terrain_revision: 7,
            ..default()
        };
        let registry = BuildingPresetRegistry {
            catalog: Handle::default(),
            revision: 1,
            resolved: ResolvedBuildingPresetCatalog::default(),
            last_error: None,
        };
        let identity = current_identity(&session, &registry).expect("草稿和目录已有完整身份");
        let mut app = App::new();
        app.insert_resource(MapEditorState {
            open: true,
            ..default()
        })
        .insert_resource(session)
        .insert_resource(registry)
        .init_resource::<TerrainPreviewState>()
        .init_resource::<BuildingAuthoringState>()
        .init_resource::<BuildingPresetEditorState>()
        .add_systems(Update, advance_index_gather);

        app.update(); // 地形派生未采用时记录目标身份，但不能开始旧高度上的收集。
        let state = app.world().resource::<BuildingAuthoringState>();
        assert_eq!(state.desired_identity.as_ref(), Some(&identity));
        assert!(state.gather.is_none());

        let mut preview = app.world_mut().resource_mut::<TerrainPreviewState>();
        preview.applied_map_id = Some(map_id);
        preview.applied_revision = 7;
        preview.mesh = Some(Handle::default());
        preview.height_image = Some(Handle::default());
        drop(preview); // 下一帧读取完整采用的同代地形派生。

        app.update();
        assert_eq!(
            app.world()
                .resource::<BuildingAuthoringState>()
                .gather
                .as_ref()
                .map(|gather| gather.cursor),
            Some(128)
        ); // 身份没有变，仍必须从等待态启动索引收集。
        app.update();
        assert_eq!(
            app.world()
                .resource::<BuildingAuthoringState>()
                .gather
                .as_ref()
                .map(|gather| gather.cursor),
            Some(256)
        ); // 稳定帧继续原收集器，不反复从零开始。
    }

    #[test]
    fn unsaved_building_clearance_survives_editor_close_until_preview_handoff_or_map_change() {
        let draft =
            crate::authoring::terrain::tests::test_heightfield_draft(Arc::new(vec![0; 121]));
        let map_id = draft.map_id.clone();
        let editor = MapEditorState {
            open: false, // 回归用户观察到的退出F12后的玩法画面。
            dirty: true, // 未保存的建筑记录及可能已同步的正式根仍可见。
            ..default()
        };
        let session = EditorAuthoringSession {
            draft: Some(draft),
            revision: 7,
            placement_structure_revision: 3,
            ..default()
        };
        let registry = BuildingPresetRegistry {
            catalog: Handle::default(),
            revision: 1,
            resolved: ResolvedBuildingPresetCatalog::default(),
            last_error: None,
        };
        let identity = current_identity(&session, &registry).expect("当前草稿与目录身份完整");
        let clearance = BuildingWorldQuad::from_local_rect(
            BuildingFootprintRect::default(),
            [5.0, 0.0, 5.0],
            0.0,
            [1.0; 3],
        );
        let entry = IndexedBuilding {
            terrain_bound: false,
            id: "unsaved-house".to_owned(),
            preset_label: "未保存建筑".to_owned(),
            footprint: clearance,
            entrance_clearance: clearance,
            vegetation_clearance: clearance,
            connections: Vec::new(),
            bounds: clearance.aabb(),
            translation: [5.0, 0.0, 5.0],
            yaw_degrees: 0.0,
            scale: [1.0; 3],
            height_mode: MapPlacementHeightMode::FixedHeight,
            foundation: BuildingFoundationDescriptor::default(),
            asset_matches_preset: true,
        };
        let state = BuildingAuthoringState {
            desired_identity: Some(identity.clone()),
            active_identity: Some(identity),
            active_index: Some(Arc::new(BuildingSpatialIndex::build(vec![entry]).unwrap())),
            ..default()
        };
        let mut app = App::new();
        app.insert_resource(editor)
            .insert_resource(session)
            .insert_resource(registry)
            .insert_resource(state)
            .insert_resource(State::new(ScenePhase::Active))
            .insert_resource(ActiveDynamicMap {
                map_id: Some(map_id.clone()),
            })
            .init_resource::<BuildingVegetationClearanceState>()
            .init_resource::<BuildingPresetEditorState>()
            .add_systems(
                Update,
                (advance_index_gather, sync_vegetation_clearance_override).chain(),
            );

        app.update();
        assert_eq!(
            app.world()
                .resource::<BuildingVegetationClearanceState>()
                .snapshot_for(&map_id)
                .unwrap()
                .regions
                .len(),
            1,
        ); // 关闭F12并不让未保存建筑下方的草重新出现。

        let preview_root = app.world_mut().spawn_empty().id();
        app.world_mut().resource_mut::<MapEditorState>().dirty = false;
        app.world_mut()
            .resource_mut::<EditorAuthoringSession>()
            .placement_previews
            .insert("unsaved-house".to_owned(), preview_root);
        app.update();
        assert!(
            app.world()
                .resource::<BuildingVegetationClearanceState>()
                .snapshot_for(&map_id)
                .is_some()
        ); // 保存已写盘但正式根尚未替代预览时仍保留净空。

        app.world_mut()
            .resource_mut::<EditorAuthoringSession>()
            .placement_previews
            .clear();
        app.update();
        assert!(
            app.world()
                .resource::<BuildingVegetationClearanceState>()
                .snapshot_for(&map_id)
                .is_none()
        ); // 正式接管并清除最后预览后撤销草稿来源。

        app.world_mut().resource_mut::<MapEditorState>().dirty = true;
        app.insert_resource(State::new(ScenePhase::Loading));
        app.update();
        assert!(
            app.world()
                .resource::<BuildingVegetationClearanceState>()
                .snapshot_for(&map_id)
                .is_none()
        ); // 切图中不得重新发布旧地图的净空。
    }
}

/// 每帧固定预算收集建筑几何；完整快照随后进入唯一 CPU 构建任务。
pub(super) fn advance_index_gather(
    editor: Res<MapEditorState>,
    session: Res<EditorAuthoringSession>,
    scene: Option<Res<State<ScenePhase>>>,
    active_dynamic: Option<Res<ActiveDynamicMap>>,
    registry: Option<Res<BuildingPresetRegistry>>,
    terrain_preview: Option<Res<TerrainPreviewState>>,
    clipmap: Option<Res<DynamicClipmapGroundSurface>>,
    mut state: ResMut<BuildingAuthoringState>,
    mut building_ui: ResMut<BuildingPresetEditorState>,
) {
    let current = if authoring_index_needed(
        &editor,
        &session,
        scene.as_deref(),
        active_dynamic.as_deref(),
    ) {
        registry
            .as_deref()
            .and_then(|registry| current_identity(&session, registry))
    } else {
        None // 已无可见草稿建筑或切图后撤销旧作者索引。
    };
    if current.is_none() {
        state.retire_audit();
        state.desired_identity = None;
        state.gather = None;
        state.active_identity = None;
        state.active_index = None; // 活动索引仅为当前作者会话持有。
        state.audit_error = None;
        state.index_error = None;
        state.last_validation = None;
        state.ground_content = None;
        if state.index_task.is_none()
            && let Some((identity, retired)) = state.audit_retirement.take()
        {
            state.index_task = Some(AsyncComputeTaskPool::get().spawn(async move {
                drop(retired);
                BuildingIndexBuildResult {
                    identity,
                    result: Err("已回收取消的建筑审计".into()),
                    discarded: true,
                }
            }));
        }
        return; // 在途 CPU 任务继续自然结束并由 poll 丢弃。
    }
    let current = current.expect("上方已经处理空身份");
    let terrain_ready = session.draft.as_ref().is_none_or(|draft| {
        if draft.manifest.terrains.is_some() {
            return draft.terrain_ground.as_ref().is_some_and(|ground| {
                ground.map == draft.map_id && ground.revision == session.terrain_revision
            }); // 容器的Empty与当前作者槽的Mesh都不能代替完整多网格承托快照。
        }
        if draft.manifest.terrain.kind == feathrise_map_content::MapTerrainKind::SparseClipmap {
            return PlacementTerrainSampler::from_sources(
                draft,
                clipmap.as_deref(),
                session.terrain_revision,
            )
            .is_ok();
        }
        draft.terrain_mesh.is_none()
            || terrain_preview
                .as_deref()
                .is_none_or(|preview| preview.ready_for(&draft.map_id, session.terrain_revision))
    }); // 动态GLB修订必须先完整派生当前高度，建筑审计不能读取上一修订的R16。
    if !terrain_ready {
        state.retire_audit();
        state.desired_identity = Some(current); // 保存门明确看到当前目标仍在等待。
        state.gather = None; // 旧高度收集前缀不能跨地形修订继续。
        state.active_identity = None; // 上一修订BVH不得继续授信新建筑点击。
        state.active_index = None;
        state.audit_error = None;
        state.index_error = None;
        state.last_validation = None;
        state.ground_content = None;
        if editor.tool == crate::types::EditorTool::Building {
            building_ui.report_status("正在等待当前地形修订完成高度、Mesh与碰撞派生…".to_owned());
        }
        return;
    }
    let sampler = session.draft.as_ref().and_then(|draft| {
        PlacementTerrainSampler::from_sources(draft, clipmap.as_deref(), session.terrain_revision)
            .ok()
    });
    if state.active_identity.as_ref() == Some(&current) {
        if sampler
            .as_ref()
            .is_some_and(|sampler| sampler.clipmap_content_revision() != state.ground_content)
        {
            // 作者修订未变的根、清单或浅坑换代也必须重审，旧高度前缀不能跨源复用。
            state.retire_audit();
            state.active_identity = None;
            state.active_index = None;
            state.last_validation = None;
            state.ground_content = None;
        } else if state.index_task.is_none()
            && let Some(query) = state.audit_ground.as_ref()
        {
            if let Some(sampler) = sampler.as_ref()
                && query.can_retry(sampler).unwrap_or(false)
            {
                let query = state.audit_ground.take().unwrap();
                let index = state.active_index.as_ref().unwrap().clone();
                let identity = current.clone();
                let bounds = session
                    .draft
                    .as_ref()
                    .unwrap()
                    .manifest
                    .world_bounds()
                    .unwrap();
                let sampler = sampler.clone();
                state.index_task = Some(AsyncComputeTaskPool::get().spawn(async move {
                    audit_index(identity, index, sampler, bounds, Some(query))
                })); // 只重新调度已变化的分页输入，复用唯一索引CPU槽。
            }
            return;
        }
    }
    if needs_new_gather(&state, &current) {
        state.retire_audit();
        let total = session
            .draft
            .as_ref()
            .map_or(0, |draft| draft.placements.placements.len());
        state.desired_identity = Some(current.clone());
        state.gather = Some(BuildingIndexGather {
            identity: current.clone(), // 每帧继续前重新核对。
            cursor: 0,                 // 从数组首项开始有界收集。
            total,                     // 冻结结构长度。
            entries: Vec::new(),       // 只为建筑项增长。
        });
        state.audit_error = None; // 新身份不能继续显示上一地图或上一修订的审计错误。
        state.index_error = None;
        state.ground_content = None; // 重新收集后只有新后台审计完成值可授予来源信用。
        state.last_validation = None; // 上一身份的红绿结果不能回显到新几何。
        if editor.tool == crate::types::EditorTool::Building {
            building_ui.report_status(format!("正在分帧收集建筑空间数据 · 0/{total}"));
        }
    }
    if state
        .index_error
        .as_ref()
        .is_some_and(|(identity, _)| identity == &current)
    {
        return; // 同一确定失败身份等待真实输入修订，不在稳定帧忙重试。
    }
    let Some(gather) = state.gather.as_ref() else {
        return; // 已拥有活动索引或正在后台构建。
    };
    if gather.identity != current {
        return; // 防御同帧资源变更，下一帧会重建最新收集器。
    }
    let Some(draft) = session.draft.as_ref() else {
        return; // 当前身份建立后草稿不会在同一系统调用中消失。
    };
    let Some(registry) = registry.as_deref() else {
        return; // 目录资源短暂缺失时保留收集前缀。
    };
    let start = gather.cursor;
    let end = start
        .saturating_add(GATHER_RECORD_BUDGET_PER_FRAME)
        .min(gather.total);
    let mut chunk = Vec::new(); // 本帧最多128项，完成后一次追加到收集器。
    let mut failure = None;
    for record in &draft.placements.placements[start..end] {
        let Some(preset_id) = record.building_preset_id.as_deref() else {
            continue; // 普通资产不参与建筑占地、门口或坡差规则。
        };
        let Some(preset) = registry.resolve(preset_id) else {
            failure = Some(format!(
                "现有建筑实例{}引用未知预设{}，无法安全检查新落点",
                record.id, preset_id,
            ));
            break; // 未知占地必须失败关闭，不能把房屋当成空气。
        };
        let footprint = preset.world_footprint(
            record.translation, // 当前作者世界位置。
            record.yaw_degrees, // 当前作者 Y 轴旋转。
            record.scale,       // 现有非均匀缩放必须进入真实占地。
        );
        let entrance_clearance = preset.world_entrance_clearance(
            record.translation, // 与占地相同世界根。
            record.yaw_degrees, // 入口方向叠加实例旋转。
            record.scale,       // 门槛与净空随实例缩放。
        );
        let vegetation_clearance = BuildingWorldQuad::from_local_rect(
            preset.vegetation_clearance, // 植被净空独立于实体占地和门前空间。
            record.translation,          // 与正式模型使用同一世界根。
            record.yaw_degrees,          // 旋转后仍保持精确四边形。
            record.scale,                // 当前记录缩放进入最终可重建净空。
        );
        let connections: Vec<_> = preset
            .connections
            .iter()
            .map(|connection| {
                let (position, outward) = preset.world_connection_marker(
                    connection,
                    record.translation,
                    record.yaw_degrees,
                    record.scale,
                ); // 连接点与正式模型使用同一S→R→T变换。
                IndexedBuildingConnection {
                    id: connection.id.clone(),     // 冻结当前目录修订的稳定点身份。
                    kind: connection.kind.clone(), // 兼容类型不在视口帧临时解析。
                    position,                      // 世界Y只供Gizmo显示。
                    outward,                       // 吸附后源方向会与其相反。
                    outward_yaw_degrees: (record.yaw_degrees + connection.outward_yaw_degrees)
                        .rem_euclid(360.0), // 目录与实例都已限制九十度档位。
                }
            })
            .collect(); // 每项已受公共格式32点上限约束。
        let mut bounds = union_rect(footprint.aabb(), entrance_clearance.aabb());
        for connection in &connections {
            let point = Vec2::new(connection.position.x, connection.position.z);
            bounds = union_rect(bounds, Rect::from_corners(point, point)); // 远离占地的合法连接点仍能进入BVH查询。
        }
        chunk.push(IndexedBuilding {
            terrain_bound: record.terrain_binding.is_some(),
            id: record.id.clone(),              // 只复制稳定短身份用于诊断和自身排除。
            preset_label: preset.label.clone(), // 后台审计返回作者可读名称。
            footprint,                          // 精确实体四边形。
            entrance_clearance,                 // 精确门前四边形。
            vegetation_clearance,               // 不修改任何作者R8的派生净空。
            connections,                        // 地面平面连接点加入同代不可变索引。
            bounds,                             // BVH同时覆盖占地、门区和可在其外侧的连接点。
            translation: record.translation,    // 审计核对五点地基最终Y。
            yaw_degrees: record.yaw_degrees,    // 审计核对九十度档位。
            scale: record.scale,                // 审计拒绝首期尚未支持的缩放。
            height_mode: record.height_mode,    // 建筑不得退回普通原点跟随。
            foundation: preset.foundation,      // 地基容差与基准冻结在同一目录修订。
            asset_matches_preset: record.asset_path == preset.asset_path
                && record.asset_target == preset.asset_target, // 模型与语义必须来自同一预设。
        });
    }
    if let Some(error) = failure {
        state.gather = None;
        state.ground_content = None;
        state.index_error = Some((current.clone(), error.clone()));
        let message = format!("建筑空间数据无效：{error}");
        building_ui.report_status(message);
        return;
    }
    let Some(gather) = state.gather.as_mut() else {
        return; // 上方失败分支已经清除收集器。
    };
    gather.entries.extend(chunk); // 单帧追加量受固定预算限制。
    gather.cursor = end;
    if editor.tool == crate::types::EditorTool::Building && end < gather.total {
        building_ui.report_status(format!("正在分帧收集建筑空间数据 · {end}/{}", gather.total));
    }
    if gather.cursor < gather.total || state.index_task.is_some() {
        return; // 未收集完，或旧 CPU 任务尚未自然结束。
    }
    let sampler = match PlacementTerrainSampler::from_sources(
        draft,
        clipmap.as_deref(),
        session.terrain_revision,
    ) {
        Ok(sampler) => sampler,
        Err(error) => {
            if draft.manifest.terrains.is_some()
                || draft.manifest.terrain.kind == feathrise_map_content::MapTerrainKind::SparseClipmap
            {
                return;
            } // 新网格/高度页尚未采用只是异步空窗，不固化同身份的不可重试错误。
            state.ground_content = None;
            state.index_error = Some((current, error.clone()));
            state.gather = None;
            building_ui.report_status(format!("建筑地基索引无法建立：{error}"));
            return;
        }
    };
    let completed_gather = state.gather.take().expect("当前收集器已经确认完整");
    let identity = completed_gather.identity;
    let entries = completed_gather.entries; // 完整轻量快照移入后台，不再复制。
    let Some(map_bounds) = draft.manifest.world_bounds() else {
        state.ground_content = None;
        state.index_error = Some((identity, "当前地图世界范围无效".to_owned()));
        building_ui.report_status("建筑地基索引无法建立：当前地图世界范围无效".to_owned());
        return;
    };
    building_ui.report_status(format!(
        "已收集{}栋建筑，正在后台构建空间索引…",
        entries.len(),
    ));
    let retired = state.audit_retirement.take();
    state.index_task = Some(AsyncComputeTaskPool::get().spawn(async move {
        drop(retired); // 复用原索引槽，既不增加并发也不保留多代完整点集。
        match BuildingSpatialIndex::build(entries) {
            Ok(index) => audit_index(identity, Arc::new(index), sampler, map_bounds, None),
            Err(error) => BuildingIndexBuildResult {
                identity,
                result: Err(error),
                discarded: false,
            },
        }
    }));
}

/// 构建与续接共用同一个worker；只保存每栋五点，不要求全地图作者页同时驻留。
fn audit_index(
    identity: BuildingIndexIdentity,
    index: Arc<BuildingSpatialIndex>,
    sampler: PlacementTerrainSampler,
    bounds: Rect,
    query: Option<BuildingGroundQuery>,
) -> BuildingIndexBuildResult {
    if sampler.clipmap_content_revision().is_none() {
        let audit_error = super::validation::audit_existing_buildings(&index, &sampler, bounds);
        return BuildingIndexBuildResult {
            identity,
            result: Ok(BuildingIndexSnapshot {
                index,
                audit_error,
                ground_query: None,
                ground_content: None,
            }),
            discarded: false,
        };
    }
    let mut query = query.unwrap_or_else(|| {
        BuildingGroundQuery::new(
            &sampler,
            (0..index.len())
                .filter_map(|slot| index.entry(slot))
                .filter(|entry| {
                    !sampler
                        .clone()
                        .for_selected_surface(Vec3::new(
                            entry.translation[0],
                            entry.translation[1] + entry.foundation.model_base_y * entry.scale[1],
                            entry.translation[2],
                        ))
                        .is_volume_layer()
                })
                .flat_map(|entry| entry.footprint.foundation_samples()),
        )
    });
    let ground_content = query.content();
    let (audit_error, ground_query) = match query.prepare(&sampler) {
        Ok(Some(frozen)) => (
            super::validation::audit_existing_buildings(&index, &frozen, bounds),
            None,
        ),
        Ok(None) => (None, Some(query)),
        Err(error) => (Some(error), None),
    };
    BuildingIndexBuildResult {
        identity,
        result: Ok(BuildingIndexSnapshot {
            index,
            audit_error,
            ground_query,
            ground_content,
        }),
        discarded: false,
    }
}
