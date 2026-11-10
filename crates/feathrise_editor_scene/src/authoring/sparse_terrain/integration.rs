//! F12 新图稀疏高度接线：单飞后台任务、笔刷收口、局部保存及异步历史恢复。
//!
//! 本模块不接触旧 GLB 作者源；所有任务都绑定 SceneId 与票据，切换场景后的迟到结果只能丢弃。

use std::collections::BTreeMap; // 一次按下到松开按全局格点合并首次值和最终值。
use std::sync::Arc; // 后台冻结的局部块由编辑器和场景共享，不在主线程复制高度数组。

use bevy::prelude::*; // ECS 资源、输入、时间和查询。
use bevy::tasks::{AsyncComputeTaskPool, IoTaskPool, Task, futures::check_ready}; // IO 与格点计算分池执行。
use bevy::ui::RelativeCursorPosition; // 左键只由中央视口消费。
use feathrise_scene_content::{
    SceneId, SceneTerrainKind, SparseHeightLiveOverride, SparseHeightTile, SparseHeightTileKey,
}; // 新格式身份、场景类型门、块坐标与未保存局部块共享覆盖。

use super::{
    SparseTerrainAuthorSession, SparseTerrainBrushInput, SparseTerrainBrushKind,
    SparseTerrainHeightChange,
}; // 独立编辑器与 F12 共用同一局部作者接口。
use crate::authoring::{EditorAuthoringSession, EditorCommand, GlobalEnvironmentAuthoringState}; // 统一历史与保存门。
use crate::components::SceneEditorViewport; // 防止 UI 外左键误雕刻。
use crate::resources::SceneEditorState; // 当前工具、光标和可见状态。
use crate::types::EditorTool; // 抬升、压低、平滑和压平共享同一稀疏雕刻入口。
use crate::viewport::VegetationGroundCursor; // 复用单飞精确光标，雕刻不再消费通用零平面落点。

#[path = "building_history.rs"]
mod building_history; // 联合历史在同一稀疏任务槽构造页候选，建筑与高度只能共同采用。
pub(crate) use building_history::poll_sparse_building_history;
#[path = "history.rs"]
mod history; // 历史按对象和源路径定向恢复，继续使用同一后台作者槽。
pub(crate) use history::apply_sparse_terrain_undo_redo;

/// 后台任务完成值始终携带原作者会话；失败不会丢失已载入或未保存的局部块。
struct SparseTerrainTaskOutput {
    ticket: u64,                                      // 切换场景或重载后旧票据失效。
    scene_id: SceneId,                                    // 同票据仍必须复核场景身份。
    author: Option<SparseTerrainAuthorSession>,       // 载入失败或纯退役任务不携带作者。
    context: SparseTerrainTaskContext,                // 采用阶段决定历史与保存语义。
    result: Result<SparseTerrainTaskSuccess, String>, // 只有完整成功才发布修改。
}

/// 单飞槽中的操作上下文；失败时仍足以恢复 UI 和历史责任。
enum SparseTerrainTaskContext {
    Load, // 从小型清单建立按需作者会话。
    CursorPrefetch {
        key: SparseHeightTileKey, // 主命中块用于失败背压，不按浮点光标位置无限重试。
    },
    Brush, // 一次局部 dab，整笔历史由主线程继续合并。
    Replay {
        undoing: bool,                                // true 从 after 恢复 before。
        changes: Arc<Vec<SparseTerrainHeightChange>>, // 与栈顶共享完整笔划，不复制大数组。
        target: super::types::SparseHistoryTarget,
        author_revision: u64,
        terrain_revision: u64,
    },
    BuildingReplay(super::types::SparseBuildingHistoryRequest),
    ReplayRollback, // 取消的私有候选先在原槽恢复旧页，再归还作者源。
    RetireReplay,   // 大量旧页只在后台释放，完成前继续占用同一任务槽。
    Save,           // 只写脏块及最后的版本化清单。
}

/// 成功结果只携带采用所需的小型元数据和局部差分。
enum SparseTerrainTaskSuccess {
    Loaded,
    CursorPrefetched, // 至多四个世界采样邻块已经完整载入作者会话。
    Brushed {
        changes: Vec<SparseTerrainHeightChange>, // 当前 dab 的实际量化变化。
        dirty_tiles: usize,                      // 当前会话实际脏块总数，不统计隐式空白全图。
        flatten_target_meters: Option<f32>,      // 首次压平任务解析出的真实高度。
        tiles: Vec<Arc<SparseHeightTile>>,       // 触及块的完整同代快照，含撤销后平地。
    },
    Replayed {
        tiles: Vec<Arc<SparseHeightTile>>, // 历史恢复后的局部块快照。
        rollback: feathrise_scene_content::SparseTerrainEditRollback,
    },
    ReplayRolledBack,
    ReplayRetired,
    BuildingReplayed(feathrise_scene_content::SparseTerrainPreparedEdit),
    Saved {
        dirty_tiles: usize,
    }, // 诊断显示本次真正提交的局部块数。
}

/// 当前一次鼠标按压的有界差分聚合；不复制任何完整高度图。
#[derive(Default)]
struct SparseTerrainStroke {
    active: bool,                       // 按下后直到已提交的单飞刷印完成。
    release_requested: bool,            // 松开可能先于后台任务完成。
    tool: Option<EditorTool>,           // 切工具必须收口原笔，不能把等待结果交给新工具。
    input_authority: Option<u64>,       // 显式网格输入被run_if跳过后，取消票据仍能阻止旧按压复活。
    flatten_target_meters: Option<f32>, // 首个 dab 从真实作者高度冻结，整笔复用。
    changes: BTreeMap<(u64, u64), SparseTerrainHeightChange>, // 同格点首次 before 和最终 after。
}

/// F12 新图稀疏作者状态；作者会话在后台任务运行期间临时移入单飞槽。
#[derive(Resource, Default)]
pub(crate) struct SparseTerrainAuthoringState {
    desired_scene_id: Option<SceneId>, // 当前保留草稿只有 SparseClipmap 才设置；关闭F12不结束作者会话。
    desired_source: Option<(Option<feathrise_scene_content::TerrainId>, String)>, // 同图对象的作者会话还必须匹配源命名空间和版本路径。
    ticket: u64,                                                          // 每次目标场景变化递增。
    author: Option<SparseTerrainAuthorSession>, // 空闲时唯一可写作者会话。
    task: Option<Task<SparseTerrainTaskOutput>>, // 载入、笔刷、历史和保存严格串行。
    stroke: SparseTerrainStroke,                // 当前按压的局部历史聚合。
    pending_save: bool,                         // 用户保存请求等待当前 dab 完整收口。
    deferred_history_undoing: Option<bool>,     // 笔划结束前暂存一次撤销或重做方向。
    load_failed_for_ticket: bool,               // 确定失败不在稳定帧无限重新读取同一坏清单。
    failed_cursor_prefetch: Option<SparseHeightTileKey>, // 同一坏块在光标离开前不再逐帧请求 IO。
    live_publish_failed: bool, // 作者已变化但实时页发布失败时，旧live与旧GPU相等也不能恢复写权限。
    external_lease: bool, // 建筑单飞槽暂借原会话时，不重新加载或并发雕刻/保存。
    building_history_ready: Option<super::types::SparseBuildingHistoryReady>,
}

impl SparseTerrainAuthoringState {
    /// 全局地形修订还覆盖姿态与LOD；高度写入只接受当前完整画布实际发布的同源live内容。
    fn current_live_height_revision(
        &self,
        session: &EditorAuthoringSession,
        live: &SparseHeightLiveOverride,
    ) -> Option<u64> {
        let draft = session.draft.as_ref()?;
        if self.live_publish_failed || self.desired_scene_id.as_ref() != Some(&draft.scene_id)
            || live.scene_id() != Some(&draft.scene_id)
            || self.author.as_ref().is_none_or(|author| author.scene_id() != &draft.scene_id)
            || draft.terrain_manifest().terrain.kind != SceneTerrainKind::SparseClipmap {
            return None;
        }
        let path = draft.terrain_manifest().terrain.height_path.as_ref()?;
        if draft.manifest.terrains.is_some() {
            let active = draft.terrain_documents.active.as_ref()?;
            let (namespace, source) = self.desired_source.as_ref()?;
            if draft.terrain_documents.loading || active.manifest.id != draft.scene_id
                || active.definition.terrain.height_path.as_ref() != Some(path)
                || source != path
                || feathrise_scene_content::sparse_height_manifest_terrain_id(&draft.scene_id, path).ok().as_ref() != Some(namespace)
                || namespace.as_ref().is_some_and(|id| id != &active.definition.id)
                || (namespace.is_none() && active.definition.id != feathrise_scene_content::TerrainId::legacy()) {
                return None;
            }
        } else if self.desired_source.is_some()
            || path != &feathrise_scene_content::sparse_height_manifest_asset_path(&draft.scene_id) {
            return None;
        }
        Some(live.revision())
    }

    pub(crate) fn current_height_snapshot<'a>(
        &self,
        session: &EditorAuthoringSession,
        live: &SparseHeightLiveOverride,
        surface: &'a feathrise_scene::TerrainObjectClipmapGround<'_>,
    ) -> Option<&'a feathrise_scene::ClipmapGroundSnapshot> {
        let revision = self.current_live_height_revision(session, live)?;
        let draft = session.draft.as_ref()?;
        let terrain = draft.manifest.terrains.as_ref().and_then(|_|
            draft.terrain_documents.active.as_ref().map(|active| &active.definition.id));
        surface.authoring_snapshot(&draft.scene_id, terrain,
            draft.terrain_manifest().terrain.height_path.as_deref()?, revision)
    }

    /// 标签暂停只允许完整作者会话；在途任务或外借整平源仍需非阻塞等待收口。
    pub(crate) fn document_can_park(&self) -> bool {
        self.task.is_none()
            && !self.stroke.active
            && !self.pending_save
            && !self.external_lease
            && self.building_history_ready.is_none()
    }

    /// 单栋整平复用当前唯一作者源；忙时保留已受理建筑请求，不另开作者会话。
    pub(crate) fn take_building_author(&mut self) -> Option<super::SparseTerrainAuthorLease> {
        if self.task.is_some()
            || self.external_lease
            || self.stroke.active
            || self.pending_save
            || self.building_history_ready.is_some()
        {
            return None;
        }
        let author = self.author.take()?;
        self.external_lease = true;
        Some(super::SparseTerrainAuthorLease {
            ticket: self.ticket,
            author,
        })
    }

    pub(crate) fn return_building_author(&mut self, lease: super::SparseTerrainAuthorLease) {
        if lease.ticket == self.ticket
            && self.desired_scene_id.as_ref() == Some(lease.author.scene_id())
        {
            self.external_lease = false;
            self.author = Some(lease.author); // 即使落点失败或被取消，也归还未修改的原作者会话。
        }
    }

    pub(crate) fn install_building_edit(
        &mut self,
        edit: feathrise_scene_content::SparseTerrainPreparedEdit,
    ) -> Result<feathrise_scene_content::SparseTerrainAppliedEdit, String> {
        if self.task.is_some() || self.external_lease || self.stroke.active {
            return Err("稀疏作者源仍被其他任务占用".into());
        }
        self.author
            .as_mut()
            .ok_or("稀疏作者源尚未就绪")?
            .install_prepared_edit(edit)
    }

    pub(crate) fn rollback_building_edit(
        &mut self,
        rollback: feathrise_scene_content::SparseTerrainEditRollback,
    ) -> Result<(), String> {
        self.author
            .as_mut()
            .ok_or("稀疏作者源已失效")?
            .rollback_prepared_edit(rollback)
    }
    /// 供尚未接入精确查询的通用工具读取驻留高度；雕刻改用共享已采用曲面，缺页不回退平面。
    pub(crate) fn sample_loaded_world_height(&self, x: f64, z: f64) -> Option<f32> {
        self.author
            .as_ref()
            .and_then(|author| author.sample_world_height(x, z).ok().flatten())
    }

    /// 透视视口用真实射线迭代已驻留稀疏高度，避免先打零平面再只改 Y 造成 X/Z 偏移。
    pub(crate) fn intersect_loaded_height_ray(
        &self,
        origin: Vec3,
        direction: Vec3,
    ) -> Option<Vec3> {
        self.author.as_ref().and_then(|author| {
            author
                .intersect_loaded_height_ray(
                    origin.as_dvec3().to_array(),
                    direction.as_dvec3().to_array(),
                )
                .map(Vec3::from_array)
        })
    }
}

/// 草稿换代后启动稀疏清单载入；不会枚举或读取全部实际高度块。
pub(crate) fn sync_sparse_terrain_authoring(
    mut state: ResMut<SparseTerrainAuthoringState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut editor: ResMut<SceneEditorState>,
    mut live: ResMut<SparseHeightLiveOverride>,
    mut retirement: ResMut<crate::authoring::AuthoringRetirement>,
) {
    let desired = session.draft.as_ref().and_then(|draft| {
        (draft.terrain_manifest().legacy_terrain().kind == SceneTerrainKind::SparseClipmap)
            .then(|| draft.scene_id.clone())
    }); // F12关闭只隐藏界面；同图未保存高度仍由玩法渲染和碰撞消费。
    let desired_source = session
        .draft
        .as_ref()
        .filter(|draft| draft.manifest.terrains.is_some())
        .and_then(|draft| {
            let path = draft.terrain_manifest().terrain.height_path.clone()?;
            let id =
                feathrise_scene_content::sparse_height_manifest_terrain_id(&draft.scene_id, &path).ok()?;
            Some((id, path))
        });
    if state.desired_scene_id != desired || state.desired_source != desired_source {
        let same_primary_source = desired.as_ref().is_some_and(|map| {
            state.desired_scene_id.as_ref() == Some(map)
                && matches!((&state.desired_source, &desired_source),
                    (None, Some((None, path))) | (Some((None, path)), None)
                    if path == &feathrise_scene_content::sparse_height_manifest_asset_path(map))
        });
        if same_primary_source {
            // 旧单地形与显式primary指向同一主清单。仅认领身份，保留脏页、在途笔划和实时覆盖。
            state.desired_source = desired_source;
        } else {
            state.ticket = state.ticket.wrapping_add(1); // 任何真实源换代的旧任务完成值立刻失效。
            state.desired_scene_id = desired.clone();
            state.desired_source = desired_source;
            if let Some(author) = state.author.take() {
                retirement.push(author); // 米制页仍可能覆盖许多脏块；换源只移动所有权，完整析构沿原回收槽后台执行。
            }
            retirement.push(std::mem::take(&mut state.stroke)); // 跨场景笔划不形成历史，大差分同样不在输入帧析构。
            state.pending_save = false; // 原图保存责任由切换场景关闭门处理。
            state.deferred_history_undoing = None; // 上一图按钮请求不能应用到新图历史。
            state.load_failed_for_ticket = false; // 新票据允许一次新的受检载入。
            state.failed_cursor_prefetch = None; // 失败块身份不能跨场景阻止合法预取。
            state.live_publish_failed = false;
            state.external_lease = false; // 旧借用完成值按票据丢弃，不能归还到新图。
            if let Some(ready) = state.building_history_ready.take() {
                retirement.push(ready);
                session.saving = false; // 已完成候选没有在途任务再归还输入门，切换场景必须在此释放。
            }
            if live.scene_id() != desired.as_ref() && live.tile_count() > 0 {
                // 完整持久索引最后一个Arc也可能拥有全图子树；同原BTreeMap一起移交既有全域回收槽。
                retirement.push(std::mem::take(&mut *live)); // 只移动所有权，不在清图帧遍历或递归释放全部旧节点。
            }
            live.retarget(desired.clone(), session.terrain_revision); // 空索引切换场景；同SceneId的画布接管仍保留已发布页。
        }
    }
    let Some(scene_id) = desired else {
        return;
    };
    if state.author.is_some()
        || state.task.is_some()
        || state.external_lease
        || state.building_history_ready.is_some()
        || state.load_failed_for_ticket
    {
        return; // 单飞槽空闲后再为当前目标启动一次载入。
    }
    let ticket = state.ticket;
    let root = crate::authoring::workspace_asset_root(); // 路径解析在后台任务中执行实际文件 IO。
    let task_scene_id = scene_id.clone();
    let source = state.desired_source.clone();
    state.task = Some(IoTaskPool::get().spawn(async move {
        let loaded = if let Some((id, path)) = source {
            SparseTerrainAuthorSession::load_source(&root, &task_scene_id, id.as_ref(), &path)
        } else {
            SparseTerrainAuthorSession::load(&root, &task_scene_id)
        };
        match loaded {
            Ok(author) => SparseTerrainTaskOutput {
                ticket,
                scene_id: task_scene_id,
                author: Some(author),
                context: SparseTerrainTaskContext::Load,
                result: Ok(SparseTerrainTaskSuccess::Loaded),
            },
            Err(error) => SparseTerrainTaskOutput {
                ticket,
                scene_id: task_scene_id,
                author: None,
                context: SparseTerrainTaskContext::Load,
                result: Err(error),
            },
        }
    }));
    editor.status = format!("正在后台载入场景{scene_id}的稀疏高度清单");
}

/// 非阻塞采用后台完成值；场景、票据与当前草稿必须同时匹配。
pub(crate) fn poll_sparse_terrain_task(
    mut state: ResMut<SparseTerrainAuthoringState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut editor: ResMut<SceneEditorState>,
    global: Res<GlobalEnvironmentAuthoringState>,
    mut live: ResMut<SparseHeightLiveOverride>,
) {
    let Some(task) = state.task.as_mut() else {
        return;
    };
    let Some(output) = check_ready(task) else {
        return;
    };
    state.task = None;
    if matches!(&output.context, SparseTerrainTaskContext::RetireReplay) {
        return; // 此完成值只有小型身份，已在后台释放的旧页不再受当前场景身份影响。
    }
    let current_scene = session.draft.as_ref().map(|draft| &draft.scene_id);
    if output.ticket != state.ticket
        || state.desired_scene_id.as_ref() != Some(&output.scene_id)
        || current_scene != Some(&output.scene_id)
    {
        if matches!(
            &output.context,
            SparseTerrainTaskContext::Replay { .. }
                | SparseTerrainTaskContext::BuildingReplay(_)
                | SparseTerrainTaskContext::ReplayRollback
                | SparseTerrainTaskContext::Save
        ) {
            session.saving = false; // 防御性释放旧图任务留下的全局输入冻结门。
        }
        history::retire_output(&mut state, output);
        return; // 迟到大作者和候选在同一后台槽释放，不触碰新图会话。
    }
    let mut returned_author = output.author;
    if !matches!(&output.context, SparseTerrainTaskContext::Replay { .. }) {
        if let Some(author) = returned_author.take() {
            state.author = Some(author); // 原作者或完整回滚后的作者才进入公共槽。
        }
    }
    match (output.context, output.result) {
        (SparseTerrainTaskContext::BuildingReplay(request), result) => {
            state.building_history_ready = Some((
                request,
                result.and_then(|success| match success {
                    SparseTerrainTaskSuccess::BuildingReplayed(edit) => Ok(edit),
                    _ => Err("稀疏建筑历史完成类型不一致".into()),
                }),
            ));
        }
        (SparseTerrainTaskContext::Load, Ok(SparseTerrainTaskSuccess::Loaded)) => {
            state.load_failed_for_ticket = false;
            editor.status = format!("场景{}稀疏高度作者清单已就绪", output.scene_id);
        }
        (
            SparseTerrainTaskContext::CursorPrefetch { key },
            Ok(SparseTerrainTaskSuccess::CursorPrefetched),
        ) => {
            if state.failed_cursor_prefetch == Some(key) {
                state.failed_cursor_prefetch = None; // 资产修复后的成功结果恢复该块后续正常预取。
            }
        }
        (
            SparseTerrainTaskContext::Brush,
            Ok(SparseTerrainTaskSuccess::Brushed {
                changes,
                dirty_tiles,
                flatten_target_meters,
                tiles,
            }),
        ) => {
            if state.stroke.flatten_target_meters.is_none() {
                state.stroke.flatten_target_meters = flatten_target_meters; // 首次真实采样成为整笔固定目标。
            }
            if !changes.is_empty() {
                merge_stroke_changes(&mut state.stroke.changes, changes); // 多个 dab 保持一条撤销命令。
                session.mark_sparse_terrain_changed(&mut editor); // 仅标局部块保存责任。
                match live.publish_tiles(&output.scene_id, session.terrain_revision, tiles) {
                    Ok(count) => {
                        state.live_publish_failed = false;
                        editor.status = format!(
                            "稀疏地形高度已修改；当前{dirty_tiles}个脏块，已发布{count}个实时 Clipmap 块"
                        );
                    }
                    Err(error) => {
                        state.live_publish_failed = true;
                        editor.status = format!("稀疏地形高度已修改，但实时覆盖发布失败：{error}");
                    }
                }
            }
        }
        (
            SparseTerrainTaskContext::Replay {
                undoing,
                changes,
                target,
                author_revision,
                terrain_revision,
            },
            Ok(SparseTerrainTaskSuccess::Replayed { tiles, rollback }),
        ) => {
            let author = returned_author
                .take()
                .expect("历史候选始终保留唯一私有作者源");
            let top = if undoing {
                session.undo.last()
            } else {
                session.redo.last()
            };
            let matches_top = top.is_some_and(|command| {
                history::command_scope(command) == target.terrain.as_ref()
                    && matches!(command.unscoped(), EditorCommand::SparseTerrainChanged(current) if Arc::ptr_eq(current, &changes))
            });
            if !matches_top
                || !history::target_is_current(&target, &state, &session)
                || session.revision != author_revision
                || session.terrain_revision != terrain_revision
                || !editor.open
                || editor.close_requested
                || editor.new_scene_dialog_open
                || live.scene_id() != Some(&output.scene_id)
                || live.revision() > session.terrain_revision
            {
                history::rollback_candidate(&mut state, author, rollback, tiles);
                session.saving = true; // 原页恢复完成前仍禁止作者修改、保存和停放。
                editor.status = "稀疏历史已取消或失效；正在后台恢复原作者页，撤销栈保持原样".into();
                return;
            }
            state.author = Some(author); // 身份完整后才发布恢复后的作者，不暴露失效候选。
            session.saving = false;
            let command = if undoing {
                session.pop_undo()
            } else {
                session.redo.pop()
            }
            .expect("已验证稀疏命令仍是栈顶");
            if undoing {
                session.redo.push(command);
            } else {
                session.undo.push(command);
            }
            session.mark_sparse_terrain_changed(&mut editor); // 恢复也产生新的作者代次和保存责任。
            let action = if undoing { "撤销" } else { "重做" };
            match live.publish_tiles(&output.scene_id, session.terrain_revision, tiles) {
                Ok(count) => {
                    state.live_publish_failed = false;
                    editor.status = format!("已{action}上一笔稀疏地形操作；已发布{count}个实时块");
                }
                Err(error) => {
                    state.live_publish_failed = true;
                    editor.status =
                        format!("已{action}上一笔稀疏地形操作，但实时覆盖发布失败：{error}");
                }
            }
            history::retire_rollback(&mut state, rollback);
        }
        (
            SparseTerrainTaskContext::ReplayRollback,
            Ok(SparseTerrainTaskSuccess::ReplayRolledBack),
        ) => {
            session.saving = false;
            editor.status = "稀疏历史未执行；原作者页、脏责任和撤销栈已保留".into();
        }
        (SparseTerrainTaskContext::Save, Ok(SparseTerrainTaskSuccess::Saved { dirty_tiles })) => {
            session.saving = false;
            session.sparse_terrain_dirty = false; // 块与清单事务完整成功后才清除责任。
            state.pending_save = false;
            if session.has_non_sparse_scene_changes() || global.dirty {
                editor.save_requested = true; // 下一帧继续既有 JSON/地表/全局联合保存。
                editor.status =
                    format!("已保存{dirty_tiles}个稀疏高度块；正在继续保存其余场景作者资源");
            } else {
                session.clear_history(None); // 稀疏源独立保存同样建立新历史基线，下一笔须重新绑定对象。
                editor.dirty = false;
                session.successful_save_sequence = session.successful_save_sequence.wrapping_add(1); // 只有无需后续联合保存时，稀疏块事务才独立提供完整成功回执。
                editor.status = format!("场景{}已保存（{dirty_tiles}个稀疏高度块）", output.scene_id);
            }
        }
        (context, Err(error)) => {
            if matches!(&context, SparseTerrainTaskContext::Load) {
                state.load_failed_for_ticket = true; // 等草稿/资产票据变化后再重试。
            }
            if matches!(&context, SparseTerrainTaskContext::Save) {
                session.saving = false;
                state.pending_save = false;
                session.sparse_terrain_dirty = true; // 保存失败保留明确责任和内存作者值。
                editor.dirty = true;
            }
            if matches!(&context, SparseTerrainTaskContext::Replay { .. }) {
                state.author = returned_author.take(); // 候选构造失败尚未安装，仍是原作者源。
                session.saving = false; // 历史失败保留栈顶但不能永久冻结输入。
            }
            if matches!(&context, SparseTerrainTaskContext::ReplayRollback) {
                session.saving = false;
            }
            if matches!(&context, SparseTerrainTaskContext::Brush) {
                state.stroke.release_requested = true; // 失败 dab 不无限自动重试。
            }
            if let SparseTerrainTaskContext::CursorPrefetch { key } = context {
                state.failed_cursor_prefetch = Some(key); // 同一内容地址坏页只报告一次，移动到其他块仍可继续工作。
                editor.status = format!("稀疏地形光标高度页预取失败：{error}");
            } else {
                editor.status = format!("稀疏地形后台操作失败：{error}");
            }
        }
        _ => {
            editor.status = "稀疏地形后台结果类型不一致，已拒绝采用".into();
        }
    }
}

/// 光标落到未驻留非平地页时启动一次有界 IO；下一帧视口会重新执行真实高度射线命中。
pub(crate) fn prefetch_sparse_terrain_cursor(
    mut state: ResMut<SparseTerrainAuthoringState>, // 独占作者会话只在任务启动时短暂移入单飞槽。
    session: Res<EditorAuthoringSession>,           // 保存、笔划与光标预取不能并发修改同一作者源。
    editor: Res<SceneEditorState>,                    // 使用本帧相机系统刚发布的严格场景命中点。
) {
    if !editor.open
        || session.saving
        || state.pending_save
        || state.stroke.active
        || state.task.is_some()
    {
        return; // 作者操作优先，光标预取绝不增加第二个并发任务。
    }
    let Some(draft) = session.draft.as_ref() else {
        return; // 尚无场景草稿时没有稀疏清单身份。
    };
    if editor.tool == crate::types::EditorTool::Vegetation {
        return;
    } // 植被使用正式显示窗口独立驱动的高度页，不依赖已接受鼠标点预取作者块。
    if draft.terrain_manifest().legacy_terrain().kind != SceneTerrainKind::SparseClipmap {
        return; // 旧 GLB 场景继续由真实三角索引命中。
    }
    let Some(point) = editor.cursor_world.map(|point| draft.terrain_point(point)) else {
        return; // 场景外或视口外光标不读取作者块。
    };
    let Some(author) = state.author.as_ref() else {
        return; // 清单载入或其他单飞任务期间保持当前安全托底。
    };
    if !matches!(
        author.sample_world_height(f64::from(point.x), f64::from(point.z)),
        Ok(None)
    ) {
        return; // 已驻留真实页、隐式平地或非法坐标都不需要 IO。
    }
    let Ok(key) = author.world_sample_tile_key(f64::from(point.x), f64::from(point.z)) else {
        return; // 受检严格命中正常不会进入这个防御分支。
    };
    if state.failed_cursor_prefetch == Some(key) {
        return; // 坏内容地址页在光标离开本块前只尝试一次。
    }
    let Some(author) = state.author.take() else {
        return; // 上方不可变检查后仍保留竞争防御。
    };
    let ticket = state.ticket; // 采用时同时核对当前场景和会话票据。
    let scene_id = author.scene_id().clone(); // 任务持有独立稳定场景身份。
    let sample = [f64::from(point.x), f64::from(point.z)]; // 后台只捕获固定两个标量。
    state.task = Some(IoTaskPool::get().spawn(async move {
        let mut author = author; // 唯一作者会话在 IO worker 内串行加载至多四块。
        let result = author
            .prefetch_world_sample_tiles(sample[0], sample[1])
            .map(|_| SparseTerrainTaskSuccess::CursorPrefetched); // 不把块数量复制回主线程。
        SparseTerrainTaskOutput {
            ticket,               // 旧场景完成值会在统一轮询入口丢弃。
            scene_id,               // 当前场景身份与票据缺一不可。
            author: Some(author), // 成功或失败都归还已持有的作者会话。
            context: SparseTerrainTaskContext::CursorPrefetch { key }, // 失败背压按主块记录。
            result,               // 完整载入后才让下一帧查询看到页。
        }
    }));
}

/// 四种雕刻工具只在精确光标、整刷基础页和同修订显示同时就绪后提交一次刷印。
pub(crate) fn apply_sparse_terrain_brush(
    mouse: Res<ButtonInput<MouseButton>>,
    time: Res<Time<Real>>,
    viewport: Query<&RelativeCursorPosition, With<SceneEditorViewport>>,
    mut state: ResMut<SparseTerrainAuthoringState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut editor: ResMut<SceneEditorState>,
    ground: Res<VegetationGroundCursor>,
    surface: Option<feathrise_scene::TerrainObjectClipmapGround>,
    live: Option<Res<SparseHeightLiveOverride>>,
    objects_cursor: Option<Res<crate::authoring::terrain_ground::TerrainGroundCursor>>,
    mode: Option<Res<crate::viewport::modes::ViewportModeState>>,
    mut objects_selection: Option<ResMut<crate::ui::terrain_objects::TerrainObjectsEditorState>>,
) {
    let tool = match editor.tool {
        EditorTool::Raise => Some(SparseTerrainBrushKind::Raise),
        EditorTool::Lower => Some(SparseTerrainBrushKind::Lower),
        EditorTool::Smooth => Some(SparseTerrainBrushKind::Smooth),
        EditorTool::Flatten => Some(SparseTerrainBrushKind::Flatten),
        _ => None, // 非高度工具继续由各自作者域处理。
    };
    let objects = session.draft.as_ref().is_some_and(|draft| draft.manifest.terrains.is_some());
    let input_available = !objects || (objects_cursor.as_ref().is_some_and(|cursor| cursor.input_authority().is_some())
        && mode.as_ref().is_none_or(|mode| mode.mode == crate::viewport::modes::ViewportMode::Edit)
        && ground.source_authoring_available());
    let sparse_scene = session.draft.as_ref().is_some_and(|draft| {
        draft.terrain_manifest().is_data_driven()
            && draft.terrain_manifest().terrain.kind == SceneTerrainKind::SparseClipmap
    }) && crate::authoring::terrain_documents::canvas_writable(&session, &editor);
    let hovered = viewport
        .single()
        .is_ok_and(RelativeCursorPosition::cursor_over);
    if editor.close_requested
        || state.pending_save
        || session.saving
        || !editor.open
        || !sparse_scene
        || !input_available
        || (objects && state.stroke.active && state.stroke.input_authority != objects_cursor.as_ref().and_then(|cursor| cursor.input_authority()))
        || tool.is_none()
        || (state.stroke.active && state.stroke.tool != Some(editor.tool))
        || !mouse.pressed(MouseButton::Left)
    {
        state.stroke.release_requested |= state.stroke.active;
    }
    finish_sparse_stroke_if_ready(&mut state, &mut session);
    if objects && session.draft.as_ref().is_some_and(|draft| draft.terrain_manifest().terrain.kind == SceneTerrainKind::SparseClipmap)
        && let Some(selection) = objects_selection.as_mut() {
        selection.brushing = state.stroke.active; // 整笔所有权沿用对象操作互斥门，不能在页任务在途时改姿态或删除源。
    }
    if !editor.open
        || editor.close_requested
        || state.pending_save
        || session.saving
        || !sparse_scene
        || !input_available
        || tool.is_none()
        || !hovered
    {
        return;
    }
    if mouse.just_pressed(MouseButton::Left) && mouse.pressed(MouseButton::Left) {
        if state.stroke.active {
            state.stroke.release_requested = true;
            return; // 上一笔仍有在途任务时不能清空其差分；本次点击等待下一次重试。
        }
        state.stroke = SparseTerrainStroke {
            active: true,
            tool: Some(editor.tool),
            input_authority: objects_cursor.as_ref().and_then(|cursor| cursor.input_authority()),
            ..default()
        }; // 起笔先记录按压；缺页期间持续按住可在就绪后开始，松手则取消未提交输入。
        if objects && let Some(selection) = objects_selection.as_mut() { selection.brushing = true; }
    }
    if !mouse.pressed(MouseButton::Left)
        || !state.stroke.active
        || state.stroke.release_requested
        || state.task.is_some()
        || state.author.is_none()
    {
        return; // 等待期间不积累旧修订刷印或时间；最新射线由公共单飞光标合并。
    }
    let Some(draft) = session.draft.as_ref() else {
        return;
    };
    let Some(snapshot) = surface.as_ref().zip(live.as_deref())
        .and_then(|(surface, live)| state.current_height_snapshot(&session, live, surface))
    else {
        editor.status = "正在等待当前作者高度正式显示；地形雕刻暂停".into();
        return;
    };
    let hit = if objects {
        let active = draft.terrain_documents.active.as_ref();
        let cursor = objects_cursor.as_deref();
        active.zip(cursor).filter(|(_, cursor)|
            cursor.identity.as_ref().is_some_and(|identity|
                    identity.scene_id == draft.scene_id && identity.revision == session.revision
                        && identity.terrain_revision == session.terrain_revision && identity.tool == editor.tool))
            .and_then(|(active, cursor)| cursor.paint_hit().filter(|hit| hit.terrain == active.definition.id).map(|hit| hit.point))
        // 按住移动只消费同按压新完成的真实作者点；不再要求它等于本帧严格hover，不拿显示保留点落笔。
    } else {
        ground.accepted_sculpt_hit(
            snapshot,
            &draft.scene_id,
            editor.tool,
            session.terrain_revision,
        ).filter(|world| editor.brush_cursor_world == Some(*world))
    };
    let Some(world) = hit.map(|point| draft.terrain_point(point))
    else {
        // 模式浮层和数值输入可在查询完成后撤销视口落点，雕刻必须继续尊重该输入屏障。
        return; // 通用光标即使带有Y=0位置也没有雕刻资格；迟到射线必须重新查询。
    };
    let kind = tool.expect("上方已验证稀疏高度工具");
    let scale = draft.terrain_documents.active.as_ref()
        .map_or(Vec3::ONE, |active| Vec3::from_array(active.definition.transform.scale));
    let footprint = [scale.x, scale.z];
    let input = SparseTerrainBrushInput {
        kind,
        center_xz: [f64::from(world.x), f64::from(world.z)],
        radius_meters: editor.brush_radius,
        strength: editor.brush_strength,
        speed_meters_per_second: editor.terrain_sculpt_speed / scale.y,
        delta_seconds: time.delta_secs().clamp(0.0, 0.05),
        flatten_target_meters: state
            .stroke
            .flatten_target_meters
            .or_else(|| (kind == SparseTerrainBrushKind::Flatten).then_some(world.y)),
    }; // 压平首笔冻结基础作者三角面的Y，而非零平面或粗显示LOD高度。
    let spec = state
        .author
        .as_ref()
        .expect("上方已确认作者会话空闲")
        .spec();
    let padding = match input.smoothing_sample_steps(spec, footprint) {
        Ok(steps) => Vec2::new(
            (steps[0] as f64 * spec.actual_spacing_meters[0]) as f32,
            (steps[1] as f64 * spec.actual_spacing_meters[1]) as f32,
        ),
        Err(error) => {
            state.stroke.release_requested = true;
            finish_sparse_stroke_if_ready(&mut state, &mut session);
            editor.status = format!("地形雕刻参数无效：{error}");
            return;
        }
    }; // 与后台预取、冻结五点采样共用世界半径步长；平滑halo不能仍只检查一格。
    let extent = Vec2::splat(input.radius_meters) / Vec2::from_array(footprint) + padding;
    match snapshot.request_authoritative_rect(world.xz() - extent, world.xz() + extent) {
        Ok(true) => start_brush_task(&mut state, input, footprint),
        Ok(false) => {
            editor.status = "正在等待完整笔刷范围的基础高度页；地形雕刻暂停".into();
        }
        Err(error) => {
            state.stroke.release_requested = true;
            finish_sparse_stroke_if_ready(&mut state, &mut session);
            editor.status = format!("地形雕刻高度查询失败：{error}");
        } // 超出36页预算或坏页明确失败，不部分雕刻或静默退回平面。
    }
}

/// 顶部保存先发布稀疏块清单；成功后若还有其他作者文件，再恢复原保存请求。
pub(crate) fn start_sparse_terrain_save(
    mut state: ResMut<SparseTerrainAuthoringState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut editor: ResMut<SceneEditorState>,
    gate: super::resources::SparseBuildingSaveGate,
) {
    if gate
        .building
        .as_deref()
        .is_some_and(|state| state.validation_pending())
        || gate
            .courtyard
            .as_deref()
            .is_some_and(|state| state.save_pending())
    {
        return; // 不抢先冻结仍在作者源借用、目录写入或原成员成组中的建筑/院落操作。
    }
    if editor.save_requested && session.sparse_terrain_dirty {
        match crate::authoring::buildings::save_readiness(
            &session,
            gate.registry.as_deref(),
            gate.building.as_deref(),
            gate.surface.as_deref(),
        ) {
            crate::authoring::buildings::BuildingSaveReadiness::Ready => {}
            crate::authoring::buildings::BuildingSaveReadiness::Waiting(message) => {
                editor.status = message;
                return;
            }
            crate::authoring::buildings::BuildingSaveReadiness::Failed(message) => {
                editor.save_requested = false;
                editor.status = format!("保存未开始：{message}");
                return;
            }
        }
    }
    if editor.save_requested && session.sparse_terrain_dirty {
        editor.save_requested = false; // 防止同帧旧GLB保存分支取得 SparseClipmap 草稿。
        state.pending_save = true;
        session.saving = true; // 等最后一个 dab 收口期间冻结其他作者输入。
    }
    if !state.pending_save || state.task.is_some() {
        return;
    }
    if state.stroke.active {
        state.stroke.release_requested = true;
        finish_sparse_stroke_if_ready(&mut state, &mut session); // 保存只等待已提交刷印，不补画尚未取得同修订地面的旧输入。
        if state.stroke.active {
            return;
        }
    }
    let Some(author) = state.author.take() else {
        if state.load_failed_for_ticket {
            state.pending_save = false;
            session.saving = false;
            editor.dirty = true;
            editor.status = "稀疏高度清单载入失败，无法安全保存；请修复资产后重新进入场景".into();
            return; // 不把未知磁盘版本当作空白清单覆盖。
        }
        editor.status = "稀疏高度清单尚未载入；就绪后自动继续保存".into();
        return;
    };
    let ticket = state.ticket;
    let scene_id = author.scene_id().clone();
    state.task = Some(IoTaskPool::get().spawn(async move {
        let mut author = author;
        let result = author
            .save_dirty()
            .map(|dirty_tiles| SparseTerrainTaskSuccess::Saved { dirty_tiles });
        SparseTerrainTaskOutput {
            ticket,
            scene_id,
            author: Some(author),
            context: SparseTerrainTaskContext::Save,
            result,
        }
    }));
    editor.status = "正在后台原子保存稀疏高度块和版本化清单".into();
}

fn start_brush_task(state: &mut SparseTerrainAuthoringState, mut input: SparseTerrainBrushInput, footprint: [f32; 2]) {
    let Some(author) = state.author.take() else {
        return; // 调用方只在空闲且整刷同修订基础页就绪时进入，不排队旧修订输入。
    };
    let ticket = state.ticket;
    let scene_id = author.scene_id().clone();
    state.task = Some(IoTaskPool::get().spawn(async move {
        let mut author = author;
        let prefetch = author.prefetch_scaled_brush_tiles(input, footprint).and_then(|_| {
            if input.kind == SparseTerrainBrushKind::Flatten
                && input.flatten_target_meters.is_none()
            {
                author.prefetch_world_sample_tiles(input.center_xz[0], input.center_xz[1])?;
                input.flatten_target_meters = Some(
                    author
                        .sample_world_height(input.center_xz[0], input.center_xz[1])?
                        .ok_or("压平中心高度块预取后仍不可用")?,
                );
            }
            Ok(())
        });
        if let Err(error) = prefetch {
            return SparseTerrainTaskOutput {
                ticket,
                scene_id,
                author: Some(author),
                context: SparseTerrainTaskContext::Brush,
                result: Err(error),
            };
        }
        let flatten_target_meters = input.flatten_target_meters;
        let (author, result) = AsyncComputeTaskPool::get()
            .spawn(async move {
                let mut author = author;
                let result = author.apply_scaled_brush_loaded(input, footprint).and_then(|brush| {
                    let tiles = author.tile_snapshots_for_changes(&brush.changes)?;
                    Ok(SparseTerrainTaskSuccess::Brushed {
                        changes: brush.changes,
                        dirty_tiles: brush.dirty_tiles,
                        flatten_target_meters,
                        tiles,
                    })
                });
                (author, result)
            })
            .await;
        SparseTerrainTaskOutput {
            ticket,
            scene_id,
            author: Some(author),
            context: SparseTerrainTaskContext::Brush,
            result,
        }
    }));
}

fn merge_stroke_changes(
    stroke: &mut BTreeMap<(u64, u64), SparseTerrainHeightChange>,
    changes: Vec<SparseTerrainHeightChange>,
) {
    for change in changes {
        stroke
            .entry((change.x, change.z))
            .and_modify(|existing| existing.after = change.after) // 首次 before 保持不变。
            .or_insert(change);
    }
}

fn finish_sparse_stroke_if_ready(
    state: &mut SparseTerrainAuthoringState,
    session: &mut EditorAuthoringSession,
) {
    if !state.stroke.active || !state.stroke.release_requested || state.task.is_some() {
        return;
    }
    state.stroke.active = false;
    state.stroke.tool = None;
    let changes = std::mem::take(&mut state.stroke.changes)
        .into_values()
        .filter(|change| change.before != change.after)
        .collect::<Vec<_>>();
    if !changes.is_empty() {
        session
            .undo
            .push(EditorCommand::SparseTerrainChanged(Arc::new(changes))); // 一次鼠标按压只形成一条命令，不按刷印预算拆笔。
        session.redo.clear();
    }
    state.stroke.flatten_target_meters = None;
    state.stroke.release_requested = false;
}

#[cfg(test)]
#[path = "object_tests.rs"]
mod object_tests;

#[cfg(test)]
mod tests {
    use super::*; // 直接验证F12生命周期系统对共享实时覆盖的所有权。
    use bevy::ecs::system::RunSystemOnce; // 最小World运行生产系统并应用资源写入。
    use feathrise_scene_content::SparseHeightSpec; // 构造一块真实受检的未保存高度快照。

    use crate::authoring::terrain::tests::test_heightfield_draft; // 复用完整场景草稿夹具，仅切换地形类型。

    #[test]
    fn long_raise_stroke_undo_redo_keeps_one_command_and_publishes_complete_tiles() {
        use bevy::tasks::TaskPool;
        IoTaskPool::get_or_init(TaskPool::new);
        AsyncComputeTaskPool::get_or_init(TaskPool::new);
        let (mut world, root) = waiting_sculpt_world(EditorTool::Raise);
        let mut state = world
            .remove_resource::<SparseTerrainAuthoringState>()
            .unwrap();
        for center_xz in [[-16., -16.], [16., -16.], [-16., 16.], [16., 16.]] {
            let brush = state
                .author
                .as_mut()
                .unwrap()
                .apply_brush(SparseTerrainBrushInput {
                    kind: SparseTerrainBrushKind::Raise,
                    center_xz,
                    radius_meters: 24.,
                    strength: 1.,
                    speed_meters_per_second: 20.,
                    delta_seconds: 0.05,
                    flatten_target_meters: None,
                })
                .unwrap(); // 四个合法局部刷印组成同一次按下至松开的完整笔划。
            assert!(brush.changes.len() as u64 <= feathrise_scene_content::MAX_SPARSE_BRUSH_CANDIDATES);
            merge_stroke_changes(&mut state.stroke.changes, brush.changes);
        }
        assert!(state.stroke.changes.len() as u64 > feathrise_scene_content::MAX_SPARSE_BRUSH_CANDIDATES);
        let scene_id = state.author.as_ref().unwrap().scene_id().clone();
        let flat = state.author.as_ref().unwrap().spec().flat_height_code();
        state.stroke.active = true;
        state.stroke.release_requested = true;
        finish_sparse_stroke_if_ready(
            &mut state,
            &mut world.resource_mut::<EditorAuthoringSession>(),
        );
        assert_eq!(world.resource::<EditorAuthoringSession>().undo.len(), 1);
        let EditorCommand::SparseTerrainChanged(original) =
            &world.resource::<EditorAuthoringSession>().undo[0]
        else {
            panic!("整笔抬高应生成一条稀疏历史");
        };
        let original = original.clone(); // 启动与完成阶段都共享同一命令，不扫描差分比较栈顶。
        world.insert_resource(state);
        world.insert_resource(GlobalEnvironmentAuthoringState::default());
        let mut live = SparseHeightLiveOverride::default();
        live.retarget(Some(scene_id.clone()), 0);
        world.insert_resource(live);

        for undoing in [true, false] {
            world.resource_mut::<SceneEditorState>().undo_requested = undoing;
            world.resource_mut::<SceneEditorState>().redo_requested = !undoing;
            world
                .run_system_once(apply_sparse_terrain_undo_redo)
                .unwrap();
            assert!(world.resource::<EditorAuthoringSession>().saving);
            assert_eq!(
                world.resource::<EditorAuthoringSession>().undo.len(),
                usize::from(undoing)
            );
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
            while world
                .resource::<SparseTerrainAuthoringState>()
                .task
                .is_some()
            {
                world.run_system_once(poll_sparse_terrain_task).unwrap();
                assert!(
                    std::time::Instant::now() < deadline,
                    "整笔历史后台任务应完成"
                );
                std::thread::sleep(std::time::Duration::from_millis(1)); // 仅测试轮询，生产系统不阻塞等待。
            }
            let session = world.resource::<EditorAuthoringSession>();
            assert!(!session.saving);
            assert_eq!(session.undo.len(), usize::from(!undoing));
            assert_eq!(session.redo.len(), usize::from(undoing));
            let history = if undoing {
                session.redo.last()
            } else {
                session.undo.last()
            };
            let Some(EditorCommand::SparseTerrainChanged(changes)) = history else {
                panic!("撤销与重做必须保留同一条完整笔划");
            };
            assert!(Arc::ptr_eq(changes, &original));
            let author = world
                .resource::<SparseTerrainAuthoringState>()
                .author
                .as_ref()
                .unwrap();
            assert!(changes.iter().all(|change| {
                author.sample_grid_encoded(change.x, change.z).unwrap()
                    == Some(if undoing { flat } else { change.after })
            }));
            let live = world.resource::<SparseHeightLiveOverride>();
            assert_eq!(live.scene_id(), Some(&scene_id));
            assert_eq!(live.revision(), session.terrain_revision);
            assert_eq!(
                live.tile_count(),
                author.tile_snapshots_for_changes(changes).unwrap().len()
            );
        }
        drop(world);
        std::fs::remove_dir_all(root).unwrap(); // 只删除本测试创建的临时场景。
    }

    fn waiting_sculpt_world(tool: EditorTool) -> (World, std::path::PathBuf) {
        let (root, scene_id) = super::super::tests::test_scene(); // 独占临时场景，不触碰作者资产。
        let author = SparseTerrainAuthorSession::load(&root, &scene_id).unwrap();
        let mut draft = test_heightfield_draft(Arc::new(vec![0; 121]));
        draft.scene_id = scene_id.clone();
        draft.manifest.id = scene_id.clone();
        draft.manifest.terrain.kind = SceneTerrainKind::SparseClipmap;
        draft.manifest.terrain.glb_path.clear();
        draft.terrain_mesh = None;
        draft.terrain_editing_index = None;
        let mut editor = SceneEditorState::default();
        editor.open = true;
        editor.tool = tool;
        editor.brush_cursor_world = Some(Vec3::ZERO); // 模拟旧通用光标的零平面托底。
        let mut mouse = ButtonInput::<MouseButton>::default();
        mouse.press(MouseButton::Left);
        let mut world = World::new();
        world.insert_resource(mouse);
        world.insert_resource(Time::<Real>::default());
        world.insert_resource(editor);
        world.insert_resource(VegetationGroundCursor::default());
        world.insert_resource(EditorAuthoringSession {
            draft: Some(draft),
            ..default()
        });
        world.insert_resource(SparseTerrainAuthoringState {
            desired_scene_id: Some(scene_id),
            author: Some(author),
            ..default()
        });
        world.spawn((
            SceneEditorViewport,
            RelativeCursorPosition {
                cursor_over: true,
                normalized: Some(Vec2::splat(0.5)),
            },
        ));
        (world, root)
    }

    #[test]
    fn locked_or_hidden_legacy_sparse_canvas_does_not_begin_a_brush() {
        for hidden in [false, true] {
            let (mut world, root) = waiting_sculpt_world(EditorTool::Raise);
            {
                let mut editor = world.resource_mut::<SceneEditorState>();
                let id = feathrise_scene_content::TerrainId::legacy();
                if hidden {
                    Arc::make_mut(&mut editor.terrain_hidden).insert(id);
                } else {
                    editor.terrain_locked.insert(id);
                }
            }
            world.run_system_once(apply_sparse_terrain_brush).unwrap();
            let state = world.resource::<SparseTerrainAuthoringState>();
            assert!(!state.stroke.active);
            assert!(state.task.is_none());
            assert!(!state.author.as_ref().unwrap().is_dirty());
            drop(world);
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn sculpt_waiting_for_display_never_uses_zero_plane_and_release_does_not_paint() {
        for tool in [
            EditorTool::Raise,
            EditorTool::Lower,
            EditorTool::Smooth,
            EditorTool::Flatten,
        ] {
            let (mut world, root) = waiting_sculpt_world(tool);
            world.run_system_once(apply_sparse_terrain_brush).unwrap();
            assert!(
                world
                    .resource::<SparseTerrainAuthoringState>()
                    .stroke
                    .active
            );
            world.resource_mut::<ButtonInput<MouseButton>>().clear();
            world.run_system_once(apply_sparse_terrain_brush).unwrap(); // 持续按住仍在等待，不启动刷印。
            let state = world.resource::<SparseTerrainAuthoringState>();
            assert!(state.task.is_none());
            assert!(!state.author.as_ref().unwrap().is_dirty());
            assert!(state.stroke.changes.is_empty());
            world
                .resource_mut::<ButtonInput<MouseButton>>()
                .release(MouseButton::Left);
            world.run_system_once(apply_sparse_terrain_brush).unwrap();
            assert!(
                !world
                    .resource::<SparseTerrainAuthoringState>()
                    .stroke
                    .active
            );
            assert!(world.resource::<EditorAuthoringSession>().undo.is_empty());
            assert_eq!(
                world.resource::<EditorAuthoringSession>().terrain_revision,
                0
            );
            drop(world);
            std::fs::remove_dir_all(root).unwrap(); // 只删除本测试独占创建的临时根。
        }
    }

    #[test]
    fn switching_sculpt_tool_cancels_the_waiting_stroke_and_requires_a_new_press() {
        let (mut world, root) = waiting_sculpt_world(EditorTool::Raise);
        world.run_system_once(apply_sparse_terrain_brush).unwrap();
        world.resource_mut::<ButtonInput<MouseButton>>().clear();
        world.resource_mut::<SceneEditorState>().tool = EditorTool::Lower;
        world.run_system_once(apply_sparse_terrain_brush).unwrap();
        assert!(
            !world
                .resource::<SparseTerrainAuthoringState>()
                .stroke
                .active
        );
        assert!(
            world
                .resource::<SparseTerrainAuthoringState>()
                .task
                .is_none()
        );
        world
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Left);
        world.resource_mut::<ButtonInput<MouseButton>>().clear();
        world
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        world.run_system_once(apply_sparse_terrain_brush).unwrap();
        assert_eq!(
            world.resource::<SparseTerrainAuthoringState>().stroke.tool,
            Some(EditorTool::Lower)
        );
        assert!(
            world
                .resource::<SparseTerrainAuthoringState>()
                .stroke
                .active
        );
        drop(world);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn closing_f12_keeps_sparse_height_override_until_the_draft_changes() {
        let scene_id = SceneId::parse("MAP-sparse-close-session").unwrap();
        let mut draft = test_heightfield_draft(Arc::new(vec![0; 121]));
        draft.scene_id = scene_id.clone();
        draft.manifest.id = scene_id.clone();
        draft.manifest.terrain.kind = SceneTerrainKind::SparseClipmap;
        draft.manifest.terrain.glb_path.clear();
        draft.manifest.terrain.height_path = Some(
            "scenes/MAP-sparse-close-session/terrain/height_tiles.scene_height_manifest.json".into(),
        );
        let revision = 9;
        let spec =
            SparseHeightSpec::new([-32.0, -32.0], [32.0, 32.0], 0.125, -80.0, 160.0).unwrap();
        let tile =
            Arc::new(SparseHeightTile::blank(&spec, SparseHeightTileKey { x: 0, z: 0 }).unwrap());
        let retired_tile = Arc::downgrade(&tile);
        let mut live = SparseHeightLiveOverride::default();
        live.retarget(Some(scene_id.clone()), revision);
        live.publish_tiles(&scene_id, revision, [tile]).unwrap();
        let state = SparseTerrainAuthoringState {
            desired_scene_id: Some(scene_id.clone()),
            load_failed_for_ticket: true, // 测试只核对生命周期，不启动文件IO任务。
            ..default()
        };
        let mut editor = SceneEditorState::default();
        editor.open = false; // 复现用户从编辑器返回玩法、但没有保存的状态。
        let session = EditorAuthoringSession {
            draft: Some(draft),
            terrain_revision: revision,
            sparse_terrain_dirty: true,
            ..default()
        };
        let mut world = World::new();
        world.insert_resource(state);
        world.insert_resource(editor);
        world.insert_resource(session);
        world.insert_resource(live);
        world.init_resource::<crate::authoring::AuthoringRetirement>();

        world
            .run_system_once(sync_sparse_terrain_authoring)
            .expect("关闭F12后的作者生命周期同步应成功");

        let live = world.resource::<SparseHeightLiveOverride>();
        assert_eq!(live.scene_id(), Some(&scene_id));
        assert_eq!(live.revision(), revision);
        assert_eq!(live.tile_count(), 1); // 游戏继续取得同一未保存块，画面和碰撞不会回退磁盘版。

        world.resource_mut::<EditorAuthoringSession>().draft = None; // 模拟切换场景或草稿真正失效。
        world
            .run_system_once(sync_sparse_terrain_authoring)
            .expect("草稿失效后的覆盖清理应成功");
        let live = world.resource::<SparseHeightLiveOverride>();
        assert!(live.scene_id().is_none());
        assert_eq!(live.tile_count(), 0); // 旧图强引用不能跨场景残留。
        assert!(retired_tile.upgrade().is_some(), "旧覆盖及完整持久索引交给既有后台回收槽，不在清图帧析构");
    }
}
