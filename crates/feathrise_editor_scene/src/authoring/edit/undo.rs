//! 统一作者命令的撤销、重做与各领域专用修订推进。

use std::sync::Arc; // R8与GLB地形作者源撤销时按需取得唯一可变所有权。

use bevy::prelude::*; // ECS资源、资产句柄与预览实体命令。
use feathrise_scene::{DynamicWaterMeshOverrides, ScenePlacementInstance, ScenePlacementWaterEvidence}; // 摆放水体撤销同步活动根覆盖与非空保存证据，候选与退役根保留原快照。

use super::{attach_patch_preview, patch_preview_parent, placement_record_bounds,
    refresh_owned_portal_preview, spawn_placement_preview, spawn_portal_preview}; // 重做放置与名称撤销复用正式预览构造路径。
use crate::authoring::resources::SurfaceBrushStroke; // 共享写侧等待的地表笔划必须先完整收口才能转移历史栈。
use crate::authoring::placement_pose::placement_record_frame; // 删除方向也先证明补丁来源，不能绕过无显式对象表的旧主地形校验。
use crate::authoring::resources::{
    ByteChange, EditorAuthoringSession, EditorCommand, EditorPreviewAssets,
    GlobalEnvironmentAuthoringState, HeightChange, WaterBrushStroke, WaterMaskMaterializeTask,
}; // 全部命令继续进入同一会话双栈。
use crate::authoring::surface_layer_edit::{
    // 引入地表作者层事务所需类型和纯助手。
    SurfaceLayerEditTask,
    start_surface_materialization_if_shared, // 保持当前地表操作的数据和原子采用边界。
}; // 当前受检分支或参数组结束。
use crate::authoring::water_network::sync_water_network_connections; // 水位及完整水域快照恢复后同步连接几何。
use crate::authoring::water_paint::{
    poll_mask_materialization,
    poll_terrain_materialization,
    start_mask_materialization_if_shared,
    start_terrain_materialization_if_shared, // 长期整笔快照共享时旧高度差分也需要后台独占写侧。
}; // 大R8撤销首次写侧继续复用笔刷的single-flight后台物化门。
use crate::authoring::{
    BuildingPlacementChange, WaterFileHandoff, command_uses_bounded_water_history,
    placement_structure_differs, placement_water_differs, }; // 摆放结构使用独立修订；大水域命令重做回undo时仍服从同一内存硬门。
use crate::resources::SceneEditorState; // 撤销请求、选择钳制与用户可见状态。
use crate::ui::placement_transform::sync_placement_record_world_with_terrain; // 历史恢复与普通编辑共用地形父归属的预检和采用。
use crate::ui::placement_water::PlacementInstanceIndex; // 网格水撤销按稳定ID命中少量并存根。 // 地表差分撤销同样禁止在Input帧复制整图。

mod courtyard; // 院落整批命令在修改任何成员前完成共同身份与容量预检。
mod deletion_preparation; // 整组增删ID按固定预算捕获，完整删除表由唯一后台槽准备。
mod surface; // 地表局部差分、懒层和整层删除的原子历史单独维护。
pub(super) mod water_snapshot; // 静水整笔前后版本在完整预检后原子采用。

/// 在确认全部下标与GLB顶点存在后一次恢复河水复合笔划，避免撤销过程只成功一半。
fn apply_water_stroke_diffs(
    mask: &mut [u8],                        // 当前稳定层R8作者源。
    mut positions: Option<&mut [[f32; 3]]>, // 纯Shift命令允许没有地形顶点差分。
    mask_changes: &[ByteChange],            // R8局部前后值。
    height_changes: &[HeightChange],        // GLB顶点Y局部前后值。
    undoing: bool,                          // true取before，false取after。
) -> bool {
    let mask_indices_valid = mask_changes.iter().all(|change| change.index < mask.len()); // 先验证全部R8下标再写任何值。
    let height_indices_valid = if height_changes.is_empty() {
        true // Shift擦除命令没有GLB地形依赖。
    } else {
        positions.as_deref().is_some_and(|values| {
            height_changes
                .iter()
                .all(|change| change.index < values.len()) // GLB顶点必须存在且全部下标有效。
        })
    };
    if !mask_indices_valid || !height_indices_valid {
        return false; // 任一侧无效时两份作者源都保持命令应用前状态。
    }
    for change in mask_changes {
        mask[change.index] = if undoing {
            change.before // 撤销恢复本笔开始前R8。
        } else {
            change.after // 重做恢复本笔完成后R8。
        };
    }
    if let Some(values) = positions.as_deref_mut() {
        for change in height_changes {
            values[change.index][1] = if undoing {
                change.before // 撤销恢复本笔开始前世界Y。
            } else {
                change.after // 重做恢复本笔完成后世界Y。
            };
        }
    }
    true // 两份差分已经作为一个内存操作完整采用。
}

/// 把一条全局天体光照历史快照按撤销或重做方向应用到作者草稿。
fn apply_global_celestial_history(
    command: &EditorCommand,
    undoing: bool,
    global: &mut GlobalEnvironmentAuthoringState,
) -> Result<(), String> {
    let EditorCommand::GlobalCelestialLightingChanged { before, after, .. } = command else {
        return Err("请求应用的历史命令不属于全局天体光照".to_owned()); // 防止未来调用方误把场景命令写入全局文件。
    };
    let profile = if undoing { before } else { after }; // 两个方向都恢复完整六组属性。
    profile.validate()?; // 损坏历史快照失败关闭，不污染实时预览。
    global.draft.celestial_lighting = profile.clone().normalized(); // RGB 与其他属性作为一个快照采用。
    Ok(())
}

/// 消费顶部按钮请求并对当前会话执行撤销或重做。
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_undo_redo(
    domains: (Option<Res<crate::authoring::volume_sculpt::VolumeAuthoringState>>, Option<Res<crate::authoring::PuddleAuthoringState>>,
        Option<ResMut<crate::ui::terrain_objects::TerrainObjectsEditorState>>, Option<ResMut<feathrise_scene::TerrainObjectTransformPreview>>,
        Option<ResMut<crate::authoring::terrain_documents::TerrainDocumentTask>>,
        Option<ResMut<crate::authoring::vegetation_paint_scope::VegetationPaintState>>,
        (Option<Res<crate::authoring::resources::TerrainBrushStroke>>, Option<Res<crate::authoring::resources::VegetationBrushStroke>>,
            Option<Res<crate::authoring::sparse_terrain::SparseTerrainAuthoringState>>),
        feathrise_scene::TerrainObjectQueries,
        Local<deletion_preparation::HistoryDeletionPreparation>), // 删除准备Local并入领域组，保持Bevy系统顶层参数不超过16项。
    domain_state: (Option<Res<crate::game_authoring::scene_interaction::SceneInteractionState>>, Option<Res<crate::authoring::domain_documents::SceneDomainAdapter>>,
        Option<Res<crate::ui::vegetation_layers::VegetationBlankLayerAllocation>>), // 外部作者整笔和图层样本事务完成前保持原历史顺序。
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    preview_assets: Res<EditorPreviewAssets>,
    mut editor: ResMut<SceneEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut courtyard_state: ResMut<crate::authoring::courtyards::CourtyardAuthoringState>, // 整组根画面交给每帧固定预算的院落同步队列。
    mut global_environment: ResMut<GlobalEnvironmentAuthoringState>, // 全局命令与场景命令共享同一线性撤销栈。
    mut materialize: ResMut<WaterMaskMaterializeTask>,
    mut surface_task: ResMut<SurfaceLayerEditTask>, // 删除与地表撤销共享一个后台CPU槽，避免无界并发复制。
    surface_stroke: Res<SurfaceBrushStroke>,        // 地表待补样本完成后才能消费最新一条命令。
    stroke: Res<WaterBrushStroke>,
    root_index: Res<PlacementInstanceIndex>, // 撤销/重做不能扫描最多8192个placement根。
    placement_roots: Query<(Entity, &ScenePlacementInstance)>, // 共享活动门决定唯一可写正式批次，不能依赖分帧清理标记。
    handoff: Res<WaterFileHandoff>, // 保存候选在途时只拒绝网格水PlacementChanged。
) {
    let (external_authoring, domain_adapter, vegetation_layers) = domain_state;
    if vegetation_layers.as_ref().is_some_and(|state| state.busy()) { return; }
    if session.model_vegetation_busy { return; } // 模型笔刷先排空并入栈，不能越过尚未形成的最后一笔。
    if session.water_preset_delete_pending {
        editor.undo_requested = false;
        editor.redo_requested = false;
        return; // 删除可能移除历史中使用的预设，不能在文件提交期间恢复新的引用。
    }
    let (volume, puddles, mut terrain_objects, mut terrain_preview, mut document_task, mut vegetation_cross,
        (terrain_stroke, vegetation_stroke, sparse), terrain_queries, mut deletion_preparation) = domains;
    deletion_preparation.maintain(&session, &mut editor); // 取消后仍排空原CPU槽；不因其他领域暂忙而丢任务句柄。
    if vegetation_cross.as_ref().is_some_and(|state|state.busy()) {return;} // 排空整笔后才能消费最新一条多网格历史。
    if terrain_objects.as_ref().is_some_and(|state| state.busy()) { return; }
    if volume.as_ref().is_some_and(|state| state.busy()) { return; } // 三维后台及整笔先收口，通用历史不能越过它。
    if external_authoring.as_ref().is_some_and(|state| state.busy()) {
        return;
    } // 不弹栈，不丢撤销请求。
    if puddles.as_ref().is_some_and(|state| state.busy()) {
        return;
    } // 保留撤销请求，先完整收口积水笔划。
    if session.saving {
        editor.undo_requested = false;
        editor.redo_requested = false;
        return;
    }
    if surface_stroke.active || surface_task.is_deleting() {
        // 依据完整作者条件选择有限处理分支。
        // 原子删层或待补笔划在途时保留撤销请求。
        if editor.undo_requested || editor.redo_requested {
            // 依据完整作者条件选择有限处理分支。
            // 只有用户明确请求历史操作时发布等待状态。
            editor.status = "当前地表作者操作正在后台完成；收口后会自动继续撤销/重做".to_owned(); // 不让请求无声丢失或撤销到旧栈顶。
        } // 等待状态发布结束。
        return; // 当前命令栈和草稿都保持完整原样。
    } // 地表事务历史串行门结束。
    if stroke.active {
        if editor.undo_requested || editor.redo_requested {
            editor.status =
                "当前水笔刷段正在等待作者源并收口；完成后会自动继续撤销/重做".to_owned();
        }
        return; // 保留请求，Presentation补画完成后下一帧撤销最新形成的水命令。
    }
    poll_mask_materialization(&mut session, &mut materialize); // 完整副本只在主线程一次换指针，不逐字节复制。
    poll_terrain_materialization(&mut session, &mut materialize); // 高度差分写侧与普通地形编辑共享唯一后台槽。
    let undoing = if editor.undo_requested {
        true
    } else if editor.redo_requested {
        false
    } else {
        return;
    };
    crate::authoring::terrain_documents::scope_history(&mut session);
    let candidate = if undoing {
        session.undo.last()
    } else {
        session.redo.last()
    };
    let Some(candidate) = candidate else {
        if undoing {
            editor.undo_requested = false;
        } else {
            editor.redo_requested = false;
        }
        editor.status = "当前没有对应的撤销/重做记录".to_owned();
        return;
    };
    if matches!(candidate, EditorCommand::RootWater(_))
        && session.draft.as_ref().is_some_and(|draft| draft.terrain_documents.active.is_some()) {
        let vegetation_pending = session.vegetation_compiled_revision != session.vegetation_revision
            && session.draft.as_ref().is_some_and(|draft| draft.vegetation.as_ref().is_some_and(|vegetation| vegetation.authoring.is_some())
                && (draft.terrain_documents.active_dirty || !draft.vegetation_author_dirty_layers.is_empty()));
        if terrain_stroke.as_ref().is_some_and(|stroke| stroke.active)
            || vegetation_stroke.as_ref().is_some_and(|stroke| stroke.active)
            || sparse.as_ref().is_some_and(|state| !state.document_can_park()) || vegetation_pending {
            editor.status = "当前画布作者事务尚未完成；收口后继续根水撤销/重做".into();
            return; // 与来源选择使用相同的停放边界；等待期间不消费请求和栈顶。
        }
        if session.sparse_terrain_dirty {
            editor.status = "当前高度作者源尚未保存；保存完成后继续根水撤销/重做".into();
            return; // 稀疏页不属于TerrainCanvas，切源会退役唯一作者，必须先提交尚未保存的雕刻或历史回退。
        }
        if session.draft.as_ref().is_some_and(|draft| draft.terrain_documents.scoped_through < session.undo.len()) {
            editor.status = "正在整理当前画布历史；完成后继续根水撤销/重做".into();
            return; // 每次仅整理64条；剩余私有命令取得来源前不能换到根槽。
        }
        if let Some(draft) = session.draft.as_mut() {
            crate::authoring::terrain_documents::root_water::focus(draft);
        }
        if let Some(selection) = terrain_objects.as_deref_mut() {
            selection.selected = None; selection.selected_index = None;
        }
        if let Some(task) = document_task.as_deref_mut() { task.requested = None; }
        editor.terrain_canvas_target = None;
        session.revision = session.revision.wrapping_add(1);
        session.water_revision = session.water_revision.wrapping_add(1);
        editor.status = "已恢复这笔历史所属的根静水画布；下一帧继续撤销/重做".into();
        return; // 栈顶和请求保持原样，当前网格的同名水层不接收根水历史。
    }
    if let EditorCommand::TerrainCanvas { id, .. } = candidate
        && session.draft.as_ref().is_some_and(|draft| !(draft.manifest.terrains.is_none() && *id==feathrise_scene_content::TerrainId::legacy())
            && draft.terrain_documents.active.as_ref().is_none_or(|active| &active.definition.id != id)) {
        if let (Some(task), Some(selection), Some(draft)) = (document_task.as_deref_mut(), terrain_objects.as_deref_mut(), session.draft.as_ref()) {
            crate::authoring::terrain_documents::request_focus(task, selection, draft, id);
            editor.status = "正在恢复这笔历史所属的地形画布；载入后自动继续".into();
        }
        return; // 栈顶保持原样，另一对象的画布不能接收这条差分。
    }
    let candidate = candidate.unscoped();
    if matches!(candidate, EditorCommand::SparseTerrainChanged(_) | EditorCommand::BuildingSparseTerrainChanged(_) | EditorCommand::VolumeSculptChanged(_)) {
        return; // 专用稀疏后台门保留请求与栈顶，绝不先恢复建筑或误写GLB。
    }
    if matches!(candidate, EditorCommand::CourtyardBatch(_))
        && courtyard_state.visual_sync_pending()
    {
        editor.status = "上一笔院落画面正在按固定预算同步；完成后自动继续整组撤销/重做".to_owned(); // 保留请求和栈顶，防止新批次使有界队列无界增长。
        return;
    }
    let changes_deletion = matches!(candidate, EditorCommand::PlacementAdded(_) | EditorCommand::PlacementRemoved { .. })
        || matches!(candidate, EditorCommand::BuildingTerrainChanged(change)
            if matches!(change.placement, BuildingPlacementChange::Added { .. }));
    if changes_deletion && session.placement_deletions.write_shared() {
        editor.status = "删除票据正在后台准备，完成后继续撤销/重做".into();
        return; // 单体恢复同样会写表，必须保留请求与原栈顶，不能触发Arc深拷贝。
    }
    let mut prepared_deletions = match deletion_preparation.prepare(&session, &editor, undoing, candidate) {
        deletion_preparation::Preparation::Unneeded => None,
        deletion_preparation::Preparation::Ready(prepared) => Some(prepared),
        deletion_preparation::Preparation::Waiting => {
            editor.status = "正在后台准备整组删除票据，完成后继续撤销/重做".into();
            return;
        }
        deletion_preparation::Preparation::Failed(error) => {
            editor.undo_requested = false; editor.redo_requested = false;
            editor.status = format!("{}未执行：{error}；历史记录已保留", if undoing { "撤销" } else { "重做" });
            return;
        }
    };
    if matches!(candidate, EditorCommand::DomainChanged { .. }) {
        editor.undo_requested = false;
        editor.redo_requested = false;
        if let Some(adapter) = domain_adapter {
            let history = adapter.history;
            let revision = session.revision;
            let map = session.draft.as_ref().map(|draft| draft.scene_id.clone());
            commands.queue(move |world: &mut World| {
                let session = world.resource::<EditorAuthoringSession>();
                let valid = session.revision == revision && session.draft.as_ref().map(|draft| &draft.scene_id) == map.as_ref();
                let result = if valid { history(world, undoing) } else { Err("历史采用前场景或作者修订变化，原记录已保留".into()) };
                world.resource_mut::<SceneEditorState>().status = result.err().unwrap_or_else(|| "正在后台校验外部文档撤销引用".into());
            });
        } else { editor.status = "此历史所属作者功能尚未安装，原记录已保留".into(); }
        return; // 不弹栈，具体引用保护成功后才由领域实现采用。
    }
    let placement_water_frozen = matches!(candidate, EditorCommand::PlacementChanged { before, after, .. } if placement_water_differs(before, after))
        && session
            .draft
            .as_ref()
            .is_some_and(|draft| handoff.placement_handoff_in_flight_for(&draft.scene_id));
    if placement_water_frozen {
        if undoing {
            editor.undo_requested = false;
        } else {
            editor.redo_requested = false;
        }
        editor.status = "已保存的正式摆放正在原子交接；完成前仅暂停网格水撤销/重做".to_owned();
        return; // 不弹栈也不保留请求，避免交接期间的其他作者操作改变之后要撤销的栈顶。
    }

    // 【帧关键路径】预览任务可短暂持有GLB网格，结构命令可长期持有R8 Arc快照。
    // 请求在资源重新唯一前保持为true，既不弹栈也不在Input帧触发Arc::make_mut整图复制。
    let waits_for_heights = matches!(candidate, EditorCommand::TerrainChanged(_))
        || matches!(candidate, EditorCommand::BuildingTerrainChanged(change) if !change.heights.is_empty())
        || matches!(
            candidate,
            EditorCommand::WaterStroke { height_changes, .. } if !height_changes.is_empty()
        );
    if waits_for_heights && start_terrain_materialization_if_shared(&session, &mut materialize) {
        // 快照长期引用不能仅等待释放。
        editor.status = "正在等待后台地形派生释放GLB作者源，随后自动完成撤销/重做".to_owned();
        return;
    }
    if matches!(candidate, EditorCommand::SurfaceChanged { .. }) {
        // 依据完整作者条件选择有限处理分支。
        // 只有局部差分需要可写像素，完整快照历史直接换Arc。
        match start_surface_materialization_if_shared(&session, &mut surface_task) {
            // 依据完整作者条件选择有限处理分支。
            // 共享历史和预览的整图副本只在后台准备。
            Ok(true) => {
                // 保持当前地表操作的数据和原子采用边界。
                // 当前写侧尚未完整就绪，不能弹出历史命令。
                editor.status = "正在后台准备地表撤销画布，完成后自动继续撤销/重做".to_owned(); // 保留按钮请求等待下一帧。
                return; // 输入帧不等待CPU任务。
            } // 异步等待分支结束。
            Ok(false) => {} // 两张图已独占，可以继续应用局部字节差分。
            Err(error) => {
                // 完成当前地表作者事务的受检参数或值转换。
                // 来源损坏或缺失时不持续重试同一坏命令。
                editor.undo_requested = false; // 清理失败请求，历史记录原样保留。
                editor.redo_requested = false; // 不让重做方向反复触发同一错误。
                editor.status = format!("地表撤销/重做准备失败：{error}"); // 保留原两图并明确显示具体原因。
                return; // 没有任何命令栈转移或像素修改。
            } // 写侧准备失败处理结束。
        } // 地表局部历史写侧门结束。
    } // 地表局部差分预检结束。
    if let EditorCommand::WaterStroke { layer_id, .. } = candidate {
        match start_mask_materialization_if_shared(&session, layer_id, &mut materialize) {
            Ok(true) => {
                editor.status = "正在后台准备水域R8撤销写侧，完成后自动继续撤销/重做".to_owned();
                return;
            }
            Ok(false) => {}
            Err(error) => {
                if undoing {
                    editor.undo_requested = false;
                } else {
                    editor.redo_requested = false;
                }
                editor.status = format!("水域撤销/重做准备失败：{error}");
                return;
            }
        }
    }

    if undoing {
        editor.undo_requested = false;
    } else {
        editor.redo_requested = false;
    }
    let command = if undoing {
        session.pop_undo()
    } else {
        session.redo.pop()
    };
    let Some(command) = command else {
        return; // 顶部命令已在同一独占系统中确认存在；仅作防御性保护。
    };
    let (terrain_scope, root_water_scope, command) = match command {
        EditorCommand::TerrainCanvas { id, command } => (Some(id), false, *command),
        EditorCommand::RootWater(command) => (None, true, *command),
        command => (None, false, command),
    };
    let mut placement_override_update = None; // 只有PlacementChanged需要在命令采用后重施当前Scene0材质。
    let mut placement_world_update = None; // 变换撤销及已有记录删除/恢复在借用结束后定向同步可见根。
    let mut courtyard_world_updates = None; // 整组成员先原子采用作者数组，修订发布后再排队同步画面。
    let mut portal_preview_update = None; // 名称撤销/重做在草稿借用结束后替换未保存Billboard。
    let global_environment_changed = command.is_global_environment(); // 应用后使用独立脏状态与修订推进。
    let apply_result = if global_environment_changed {
        apply_global_celestial_history(&command, undoing, &mut global_environment)
    } else {
        (|| -> Result<(), String> {
            // 容量预检只计本方向实际移除的ID；失败沿统一Err门原样回栈，不先改作者数据。
            if prepared_deletions.as_ref().is_some_and(|prepared| !prepared.matches(&session.placement_deletions)) {
                return Err("整组删除票据来源已变化".into());
            }
            let deletion_id = match &command {
                EditorCommand::PlacementAdded(record) if undoing => Some(record.id.as_str()),
                EditorCommand::PlacementRemoved { record, .. } if !undoing => Some(record.id.as_str()),
                EditorCommand::BuildingTerrainChanged(change) if undoing => match &change.placement {
                    BuildingPlacementChange::Added { record, .. } => Some(record.id.as_str()),
                    _ => None,
                },
                _ => None, // 整组容量由worker完整目标表证明，主线程不再分配/遍历全量ID集合。
            };
            if let Some(deletion_id) = deletion_id {
                crate::authoring::placement_deletion::validate_deletions(&session, std::iter::once(deletion_id))?;
            } // 纯变换、水历史和已准备整表不写原表，共享源不能误拒这些独立命令。
            let draft = session
                .draft
                .as_mut()
                .ok_or_else(|| "当前场景草稿已经失效".to_owned())?;
            match &command {
                EditorCommand::TerrainCanvas { .. } | EditorCommand::RootWater(_) => unreachable!("对象历史已在进入领域应用前解包"),
                EditorCommand::TerrainObjectsChanged { before, after } | EditorCommand::ClipmapStitchesChanged {before,after,..} => {
                    if let Some(preview) = terrain_preview.as_mut() { preview.pose = None; }
                    (if undoing { before } else { after }).apply(&mut draft.manifest);
                    crate::authoring::terrain_documents::restore_legacy(draft)?;
                    draft.terrain_ground = None;
                }
                EditorCommand::SceneObjectsChanged(change) => change.apply(&mut draft.placements, undoing)?,
                EditorCommand::SubscenesChanged { before, after } => {
                    draft.manifest.subscenes = if undoing { before.clone() } else { after.clone() };
                }
                EditorCommand::VolumeSculptChanged(_) => unreachable!("三维历史由专用后台和联合采用门处理"),
                EditorCommand::DomainChanged { .. } => unreachable!("外部文档历史由实际领域事务处理"),
                EditorCommand::GlobalCelestialLightingChanged { .. } => {
                    unreachable!("全局命令已在取得场景草稿前单独处理") // 保持枚举匹配完整且禁止误入场景路径。
                }
                EditorCommand::PlacementAdded(record) => {
                    placement_record_frame(draft, record)?;
                    if undoing {
                        let removed_surface_count = draft
                            .placements
                            .placements
                            .last()
                            .filter(|value| value.id == record.id)
                            .map(|value| value.water_surfaces.len())
                            .ok_or_else(|| {
                                "新增摆放已不在数组末尾，拒绝撤销到错误实例".to_owned()
                            })?;
                        draft.placements.placements.pop(); // 新增命令始终追加；LIFO撤销据此保持O(1)，不扫描8192条记录。
                        draft.placement_water_surface_count = draft
                            .placement_water_surface_count
                            .saturating_sub(removed_surface_count); // 结构撤销按被删记录差值维护O(1)全图水绑定预算缓存。
                        let preview_scene = draft.scene_id.clone();
                        // 陈旧所有权地址仍需撤销，但只回收身份仍属于当前场景和记录的作者根。
                        if let Some(entity) = session.placement_previews.remove(&record.id)
                            && placement_roots.get(entity).is_ok_and(|(_, instance)| {
                                instance.scene_id() == &preview_scene && instance.id() == record.id
                            }) {
                            commands.entity(entity).try_despawn();
                        }
                    } else {
                        let bounds = placement_record_bounds(draft, record)?;
                        record.validated_transform(&draft.scene_id, bounds)?;
                        patch_preview_parent(draft, &terrain_queries, record)?;
                        draft.placements.placements.push(record.clone());
                        draft.placement_water_surface_count = draft
                            .placement_water_surface_count
                            .saturating_add(record.water_surfaces.len()); // 重做未来也兼容含赋水快照的新增命令。
                        let entity = spawn_placement_preview(
                            &mut commands,
                            &asset_server,
                            &draft.scene_id,
                            record,
                        ); // 重做预览必须冻结当前草稿SceneId及完整来源记录。
                        attach_patch_preview(&mut commands, draft, &terrain_queries, entity, record)?;
                        session.placement_previews.insert(record.id.clone(), entity);
                    }
                }
                EditorCommand::PlacementRemoved {
                    index,
                    record,
                    had_author_preview,
                } => {
                    placement_record_frame(draft, record)?; // 有无作者预览都不能在来源无效时先删除记录或水绑定计数。
                    if undoing {
                        let bounds = placement_record_bounds(draft, record)?;
                        record.validated_transform(&draft.scene_id, bounds)?;
                        patch_preview_parent(draft, &terrain_queries, record)?;
                        if *index > draft.placements.placements.len()
                            || draft
                                .placements
                                .placements
                                .iter()
                                .any(|candidate| candidate.id == record.id)
                        {
                            return Err("被删资产原下标或稳定ID已失效，拒绝错误恢复".to_owned());
                        }
                        draft.placements.placements.insert(*index, record.clone()); // 恢复原顺序，场景树和后续记录下标稳定。
                        draft.placement_water_surface_count = draft
                            .placement_water_surface_count
                            .saturating_add(record.water_surfaces.len()); // 恢复含水资产同步维护O(1)全图预算缓存。
                        if *had_author_preview {
                            let entity = spawn_placement_preview(
                                &mut commands,
                                &asset_server,
                                &draft.scene_id,
                                record,
                            ); // 原本未保存的新资产由撤销重新生成作者Scene0预览。
                            attach_patch_preview(&mut commands, draft, &terrain_queries, entity, record)?;
                            session.placement_previews.insert(record.id.clone(), entity); // 稳定ID恢复精确预览所有权。
                        } else {
                            placement_world_update = Some((record.clone(), bounds, true, true)); // 已有正式根恢复可见；缺根时补作者预览。
                        }
                    } else {
                        draft
                            .placements
                            .placements
                            .get(*index)
                            .filter(|candidate| candidate.id == record.id)
                            .ok_or_else(|| {
                                "待重做删除的资产下标或稳定ID已失效，拒绝串删".to_owned()
                            })?;
                        if !*had_author_preview {
                            let bounds = placement_record_bounds(draft, record)?;
                            record.validated_transform(&draft.scene_id, bounds)?;
                        } // 保留原正式根格式门并前移到写入前，隐藏不再修改正式姿态或重新挂父。
                        draft.placements.placements.remove(*index); // 预检完成后精确移除冻结数组项。
                        draft.placement_water_surface_count = draft
                            .placement_water_surface_count
                            .saturating_sub(record.water_surfaces.len()); // 重做删除同步维护全图水绑定缓存。
                        let preview_scene = draft.scene_id.clone();
                        // 重做删除也可能遇到旧预览地址；不能连同别的场景或稳定ID拥有的根一起删除。
                        if let Some(entity) = session.placement_previews.remove(&record.id)
                            && placement_roots.get(entity).is_ok_and(|(_, instance)| {
                                instance.scene_id() == &preview_scene && instance.id() == record.id
                            }) {
                            commands.entity(entity).try_despawn(); // 未保存原根或撤销时补出的兜底预览都必须精确回收。
                        }
                    }
                }
                EditorCommand::PlacementChanged {
                    record_index,
                    before,
                    after,
                } => {
                    let desired = if undoing { before } else { after }; // 两个方向都恢复完整记录而非局部拼接水体数组。
                    let bounds = placement_record_bounds(draft, desired)?;
                    desired.validated_transform(&draft.scene_id, bounds)?;
                    patch_preview_parent(draft, &terrain_queries, desired)?;
                    let record = draft
                        .placements
                        .placements
                        .get_mut(*record_index)
                        .filter(|record| record.id == desired.id)
                        .ok_or_else(|| "摆放数组下标或稳定ID已失效，拒绝串写".to_owned())?;
                    let previous_surface_count = record.water_surfaces.len();
                    *record = desired.clone(); // 作者JSON和稍后的当前世界覆盖来自同一个完整快照。
                    draft.placement_water_surface_count = draft
                        .placement_water_surface_count
                        .saturating_sub(previous_surface_count)
                        .saturating_add(desired.water_surfaces.len()); // 类别/数值不改计数，赋水增删只应用单记录差值。
                    placement_world_update = Some((desired.clone(), bounds, true, true)); // 水体和纯变换撤销都恢复完整可见Transform。
                    if placement_water_differs(before, after) {
                        placement_override_update = Some((
                            draft.scene_id.clone(),
                            desired.id.clone(),
                            desired.water_surfaces.clone(),
                        )); // 只有真实水体变化才重施覆盖和触发动态水验证。
                    }
                }
                EditorCommand::BuildingTerrainChanged(change) => {
                    let terrain_len = if change.heights.is_empty() {
                        None // 防御空地形差分时不要求动态GLB作者源。
                    } else {
                        Some(
                            draft
                                .terrain_mesh
                                .as_ref()
                                .map(|mesh| mesh.positions.len())
                                .ok_or_else(|| "建筑联合历史所需的GLB地形已经失效".to_owned())?,
                        )
                    };
                    if terrain_len.is_some_and(|length| {
                        change.heights.iter().any(|height| height.index >= length)
                    }) {
                        return Err("建筑联合历史的地形顶点下标已经失效".to_owned());
                    }
                    if let Some(mesh) = draft.terrain_mesh.as_ref() {
                        let expected_current = |height: &HeightChange| {
                            if undoing { height.after } else { height.before }
                        };
                        if change.heights.iter().any(|height| {
                            mesh.positions[height.index][1] != expected_current(height)
                        }) {
                            return Err(
                                "建筑联合历史的当前地形值与事务起点不一致，拒绝部分恢复"
                                    .to_owned(),
                            );
                        }
                    }
                    match &change.placement {
                        BuildingPlacementChange::Added {
                            record_index,
                            record,
                        } => {
                            if undoing {
                                if *record_index + 1 != draft.placements.placements.len()
                                    || draft
                                        .placements
                                        .placements
                                        .get(*record_index)
                                        .is_none_or(|current| current != record)
                                {
                                    return Err("联合新增建筑已不在冻结尾槽，拒绝部分撤销".to_owned());
                                }
                            } else if *record_index != draft.placements.placements.len()
                                || draft
                                    .placements
                                    .placements
                                    .iter()
                                    .any(|current| current.id == record.id)
                            {
                                return Err("联合新增建筑的重做数组身份已经失效".to_owned());
                            }
                        }
                        BuildingPlacementChange::Changed {
                            record_index,
                            before,
                            after,
                        } => {
                            let expected = if undoing { after } else { before };
                            if draft
                                .placements
                                .placements
                                .get(*record_index)
                                .is_none_or(|current| current != expected)
                            {
                                return Err(
                                    "联合移动建筑的数组下标或完整起点已经失效".to_owned(),
                                );
                            }
                            draft
                                .manifest
                                .world_bounds()
                                .ok_or_else(|| "恢复联合建筑变换时场景世界范围无效".to_owned())?;
                        }
                    }

                    if !change.heights.is_empty() {
                        let mesh = Arc::make_mut(
                            draft
                                .terrain_mesh
                                .as_mut()
                                .expect("上方已完整预检联合地形作者源"),
                        ); // 共享写侧已由历史入口在后台物化，此处不会复制整图。
                        for height in &change.heights {
                            mesh.positions[height.index][1] =
                                if undoing { height.before } else { height.after };
                        }
                        draft.terrain_dirty = true; // 联合历史任一方向都需重新派生并进入保存门。
                    }
                    match &change.placement {
                        BuildingPlacementChange::Added {
                            record_index: _,
                            record,
                        } => {
                            if undoing {
                                draft.placements.placements.pop(); // 预检已证明冻结记录位于尾部。
                                draft.placement_water_surface_count = draft
                                    .placement_water_surface_count
                                    .saturating_sub(record.water_surfaces.len());
                                let preview_scene = draft.scene_id.clone();
                                // 联合建筑历史遵守同一所有权门，作者记录删除不授予外部实体回收权。
                                if let Some(entity) = session.placement_previews.remove(&record.id)
                                    && placement_roots.get(entity).is_ok_and(|(_, instance)| {
                                        instance.scene_id() == &preview_scene && instance.id() == record.id
                                    }) {
                                    commands.entity(entity).try_despawn();
                                }
                            } else {
                                draft.placements.placements.push(record.clone());
                                draft.placement_water_surface_count = draft
                                    .placement_water_surface_count
                                    .saturating_add(record.water_surfaces.len());
                                let entity = spawn_placement_preview(
                                    &mut commands,
                                    &asset_server,
                                    &draft.scene_id,
                                    record,
                                );
                                session.placement_previews.insert(record.id.clone(), entity);
                            }
                        }
                        BuildingPlacementChange::Changed {
                            record_index,
                            before,
                            after,
                        } => {
                            let desired = if undoing { before } else { after };
                            let current = &mut draft.placements.placements[*record_index];
                            let previous_surface_count = current.water_surfaces.len();
                            *current = desired.clone(); // 地形已完整写入后，建筑侧不会再遇到可失败分支。
                            draft.placement_water_surface_count = draft
                                .placement_water_surface_count
                                .saturating_sub(previous_surface_count)
                                .saturating_add(desired.water_surfaces.len());
                            let bounds = draft
                                .manifest
                                .world_bounds()
                                .expect("上方已预检联合建筑世界范围");
                            placement_world_update =
                                Some((desired.clone(), bounds, true, true));
                            if placement_water_differs(before, after) {
                                placement_override_update = Some((
                                    draft.scene_id.clone(),
                                    desired.id.clone(),
                                    desired.water_surfaces.clone(),
                                ));
                            }
                        }
                    }
                }
                EditorCommand::CourtyardBatch(change) => {
                    if let Some(transfer) = &change.model_vegetation {
                        crate::authoring::model_vegetation::validate_transfer(draft,
                            if undoing { &transfer.after } else { &transfer.before })?;
                    }
                    let bounds = draft
                        .manifest
                        .world_bounds()
                        .ok_or_else(|| "恢复院落时场景世界范围无效".to_owned())?; // 全组使用同一冻结清单范围。
                    // 先证明全部补丁来源和恢复所需父级；失败不能先删除成员或留下只有局部姿态的悬空预览。
                    for (_, record) in &change.removed {
                        placement_record_frame(draft, record)?;
                        if undoing {
                            patch_preview_parent(draft, &terrain_queries, record)?;
                        }
                    }
                    for record in &change.added {
                        placement_record_frame(draft, record)?; // 撤销整组新增同样会删除成员，来源检查须先于纯数组采用。
                        if !undoing {
                            patch_preview_parent(draft, &terrain_queries, record)?;
                        }
                    }
                    for (_, before, after) in &change.changed {
                        patch_preview_parent(draft, &terrain_queries, if undoing { before } else { after })?;
                    }
                    let updates = courtyard::apply_courtyard_batch_records_by_source(
                        &mut draft.placements.placements, // 预检通过后才一次修改全部成员。
                        &mut draft.placement_water_surface_count, // 水绑定缓存与成员数组同事务更新。
                        change, // 命令持有完整前后成员快照。
                        undoing, // 撤销移除尾段，重做按原顺序恢复。
                        &draft.scene_id, // 任何成员都不能落在另一个场景的范围上。
                        bounds, // 坏变换在整组写入前拒绝。
                        draft.manifest.terrains.as_deref(),
                    )?;
                    courtyard_world_updates = Some(updates); // 离开草稿可变借用后按固定预算同步ECS根。
                    if let Some(transfer) = &change.model_vegetation {
                        crate::authoring::model_vegetation::transfer(draft,
                            if undoing { transfer.before.clone() } else { transfer.after.clone() })?;
                    }
                }
                EditorCommand::PortalAdded(record) => {
                    if undoing {
                        draft
                            .portals
                            .portals
                            .last()
                            .filter(|value| value.id == record.id)
                            .ok_or_else(|| {
                                "新增传送点已不在数组末尾，拒绝撤销到错误记录".to_owned()
                            })?;
                        draft.portals.portals.pop(); // 传送点新增同样是追加命令，LIFO撤销无需全表retain。
                        if let Some(entity) = session.portal_previews.remove(&record.id) {
                            commands.entity(entity).try_despawn();
                        }
                        if editor.selected_current_portal_id.as_deref() == Some(record.id.as_str())
                        {
                            editor.selected_current_portal_id = None; // 撤销新增后同帧清除已不存在的当前行高亮。
                        }
                    } else {
                        draft.portals.portals.push(record.clone());
                        let entity = spawn_portal_preview(&mut commands, &preview_assets, record);
                        session.portal_previews.insert(record.id.clone(), entity);
                        editor.selected_current_portal_id = Some(record.id.clone()); // 重做新增后恢复源点高亮和自动定位页。
                    }
                }
                EditorCommand::PortalRemoved {
                    index,
                    record,
                    had_author_preview,
                } => {
                    if undoing {
                        if *index > draft.portals.portals.len()
                            || draft
                                .portals
                                .portals
                                .iter()
                                .any(|candidate| candidate.id == record.id)
                        {
                            return Err("被删传送点原下标或稳定ID已失效，拒绝错误恢复".to_owned());
                        }
                        draft.portals.portals.insert(*index, record.clone()); // 恢复原顺序，分页和其他传送点稳定不漂移。
                        if *had_author_preview {
                            let entity =
                                spawn_portal_preview(&mut commands, &preview_assets, record);
                            session.portal_previews.insert(record.id.clone(), entity); // 仅恢复原本属于未保存新增点的预览。
                        }
                        editor.selected_current_portal_id = Some(record.id.clone()); // 撤销后立即回显恢复项。
                    } else {
                        draft
                            .portals
                            .portals
                            .get(*index)
                            .filter(|candidate| candidate.id == record.id)
                            .ok_or_else(|| {
                                "待重做删除的传送点下标或稳定ID已失效，拒绝串删".to_owned()
                            })?;
                        draft.portals.portals.remove(*index); // 预检完成后只移除冻结的精确数组项。
                        if *had_author_preview
                            && let Some(entity) = session.portal_previews.remove(&record.id)
                        {
                            commands.entity(entity).try_despawn(); // 重做未保存点删除时同步回收恢复出的预览。
                        }
                        if editor.selected_current_portal_id.as_deref() == Some(record.id.as_str())
                        {
                            editor.selected_current_portal_id = None; // 重做后不留下悬空高亮。
                        }
                    }
                }
                EditorCommand::PortalLabelChanged {
                    portal_id,
                    before_label,
                    before_customized,
                    after_label,
                    after_customized,
                } => {
                    let record = draft
                        .portals
                        .portals
                        .iter_mut()
                        .find(|record| record.id == *portal_id)
                        .ok_or_else(|| "改名传送点稳定ID已失效，拒绝串写".to_owned())?;
                    if undoing {
                        record.label = before_label.clone(); // 撤销恢复原作者文字或默认快照。
                        record.label_customized = *before_customized; // 同步恢复自动命名状态。
                    } else {
                        record.label = after_label.clone(); // 重做再次应用受检作者名称。
                        record.label_customized = *after_customized; // 显式名称状态与文字原子恢复。
                    }
                    portal_preview_update = Some(record.clone()); // 借用结束后按完整记录重建可能存在的未保存预览。
                    editor.selected_current_portal_id = Some(portal_id.clone()); // 撤销/重做后继续高亮被改名的同一点。
                }
                EditorCommand::TerrainChanged(changes) => {
                    let mesh = draft
                        .terrain_mesh
                        .as_mut()
                        .ok_or_else(|| "当前草稿没有可编辑GLB地形作者源".to_owned())?;
                    if !changes
                        .iter()
                        .all(|change| change.index < mesh.positions.len())
                    {
                        return Err("地形差分下标已失效".to_owned());
                    }
                    let mesh = Arc::make_mut(mesh); // 预检已确认唯一所有权，此处不会复制整份GLB网格。
                    for change in changes {
                        mesh.positions[change.index][1] =
                            if undoing { change.before } else { change.after };
                    }
                    draft.terrain_dirty = true;
                }
                EditorCommand::SparseTerrainChanged(_) | EditorCommand::BuildingSparseTerrainChanged(_) => {
                    return Err("稀疏地形历史必须由专用后台块任务执行".to_owned()); // 调度遗漏时绝不误写旧GLB作者源。
                }
                EditorCommand::SurfaceChanged { .. } // 局部笔刷历史先通过后台写侧门。
                | EditorCommand::SurfaceLayerAdded { .. } // 懒加入引用维持原顺序和稳定ID。
                | EditorCommand::SurfaceAuthoringChanged(_) => { // 整层删除同时恢复定义、索引和权重。
                    surface::apply_surface_history(draft, &mut editor, &command, undoing)?; // 统一助手全部校验后才原子应用。
                } // 三类地表命令共享一个受检历史边界。
                EditorCommand::VegetationChanged { layer_id, changes } => {
                    let density = draft
                        .vegetation_author_densities
                        .get_mut(layer_id)
                        .ok_or_else(|| format!("植被作者层{layer_id}已经失效"))?;
                    if !changes.iter().all(|change| change.index < density.len()) {
                        return Err("植被差分下标已失效".to_owned());
                    }
                    let density = Arc::make_mut(density); // 后台编译仍可安全完成旧修订，撤销只写时复制当前作者层。
                    for change in changes {
                        density[change.index] = if undoing { change.before } else { change.after };
                    }
                    draft
                        .vegetation_author_dirty_layers
                        .insert(layer_id.clone()); // 正式输出由随后同修订后台编译统一标脏。
                }
                EditorCommand::VegetationPaintChanged(change) => {
                    crate::authoring::vegetation_paint_scope::apply_history(draft,change,undoing)?;
                }
                EditorCommand::ModelVegetationChanged { .. } => {
                    crate::authoring::model_vegetation::apply_history(draft, &command, undoing)?;
                }
                EditorCommand::VegetationAuthoringChanged(change) => {
                    let vegetation = draft
                        .vegetation
                        .as_mut()
                        .ok_or_else(|| "当前草稿的植被作者结构已经失效".to_owned())?;
                    if let Some(models) = &change.model_vegetation {
                        let (expected, desired) = if undoing {
                            (&models.after, &models.before)
                        } else {
                            (&models.before, &models.after)
                        };
                        if !Arc::ptr_eq(&vegetation.model_surfaces, expected) {
                            return Err("模型植被图层历史的目标表已失效，结构和样本均未修改".into());
                        }
                        vegetation.model_surfaces = desired.clone(); // O(1)恢复整表，不在输入帧重建样本。
                    }
                    vegetation.layers = if undoing {
                        change.before_outputs.clone() // 首次预设输出与作者画布共同撤销。
                    } else {
                        change.after_outputs.clone() // 重做使用命令快照，不受目录后来重排或热更影响。
                    };
                    vegetation.authoring = Some(if undoing {
                        change.before.clone() // 撤销原子恢复组、层顺序和参数。
                    } else {
                        change.after.clone() // 重做原子恢复同一完成结构。
                    });
                    draft.vegetation_author_densities = if undoing {
                        change.before_densities.clone() // BTreeMap只克隆Arc，不复制整套R8像素。
                    } else {
                        change.after_densities.clone() // 新增、删除或复制画布与描述同次恢复。
                    };
                }
                EditorCommand::WaterStrokeSnapshot(change) => { // 同一按压只对应一条完整命令。
                    water_snapshot::apply(draft, change, undoing)?; // 两份作者源同时预检后O(1)替换。
                }
                EditorCommand::WaterStroke {
                    layer_id,
                    mask_changes,
                    height_changes,
                } => {
                    let mask_len = draft
                        .water_masks
                        .get(layer_id)
                        .map(|mask| mask.len())
                        .ok_or_else(|| format!("水域作者层{layer_id}已经失效"))?;
                    if !mask_changes.iter().all(|change| change.index < mask_len) {
                        return Err("水域R8差分下标已失效".to_owned());
                    }
                    if !height_changes.is_empty() {
                        let height_len = draft
                            .terrain_mesh
                            .as_ref()
                            .map(|mesh| mesh.positions.len())
                            .ok_or_else(|| "复合水笔命令所需的GLB地形已经失效".to_owned())?;
                        if !height_changes
                            .iter()
                            .all(|change| change.index < height_len)
                        {
                            return Err("复合水笔GLB顶点差分下标已失效".to_owned());
                        }
                    }
                    let super::super::resources::EditorSceneDraft {
                        water_masks,
                        terrain_mesh,
                        ..
                    } = draft; // 显式拆分两份作者源，随后作为一个内存事务采用。
                    let mask = Arc::make_mut(
                        water_masks
                            .get_mut(layer_id)
                            .expect("上方已验证稳定水层仍存在"),
                    ); // 结构快照共享已在后台物化，此处不会复制整张R8。
                    let positions = if height_changes.is_empty() {
                        None // Shift擦除命令没有GLB顶点差分，不要求地形作者源。
                    } else {
                        Some(
                            Arc::make_mut(terrain_mesh.as_mut().expect("上方已验证GLB仍存在"))
                                .positions
                                .as_mut_slice(),
                        ) // 预检保证GLB唯一，再与R8原子应用边界内恢复。
                    };
                    if !apply_water_stroke_diffs(
                        mask.as_mut_slice(), // 当前层R8唯一可变切片。
                        positions,           // 可选GLB顶点唯一可变切片。
                        mask_changes,        // 本笔R8局部差分。
                        height_changes,      // 本笔GLB顶点Y局部差分。
                        undoing,             // 撤销或重做方向。
                    ) {
                        return Err("水笔复合差分预检后意外失效".to_owned());
                    }
                }
                EditorCommand::WaterLevelChanged {
                    layer_id,
                    before,
                    after,
                } => {
                    let water = draft
                        .water
                        .as_mut()
                        .ok_or_else(|| "当前草稿的水域文档已经失效".to_owned())?;
                    let layer = water
                        .layers
                        .iter_mut()
                        .find(|layer| &layer.id == layer_id)
                        .ok_or_else(|| format!("水层{layer_id}已经失效"))?;
                    layer.water_level = if undoing { *before } else { *after }; // 先恢复权威静水位。
                    sync_water_network_connections(water); // 同一撤销/重做操作派生河流首尾与瀑布静水端，不增加第二条命令。
                }
                EditorCommand::WaterLayersChanged(change) => {
                    draft.water = Some(if undoing {
                        change.before.clone() // 撤销完整恢复被删层及其独立水位。
                    } else {
                        change.after.clone() // 重做采用同一份已验证水层集合。
                    });
                    draft.water_masks = if undoing {
                        change.before_masks.clone() // Arc浅快照立即恢复被删R8，不访问磁盘。
                    } else {
                        change.after_masks.clone()
                    };
                    if let Some(water) = draft.water.as_mut() {
                        sync_water_network_connections(water); // 结构快照恢复后清理失效引用并重新吸附全部端点。
                    }
                }
                EditorCommand::WaterDefinitionsChanged { before, after } => {
                    draft.water = Some(if undoing {
                        before.clone() // 撤销原子恢复河流、瀑布、端点连接和全部控制点参数。
                    } else {
                        after.clone() // 重做采用同一份编辑期已验证完整水域快照。
                    });
                    if let Some(water) = draft.water.as_mut() {
                        sync_water_network_connections(water); // 旧快照也按当前统一规则恢复连接，不让派生端点漂移。
                    }
                }
                EditorCommand::PuddlesChanged { before, after } => { // 在完整版本间常数时间切换。
                    super::super::puddles::apply_snapshot(draft, if undoing { before } else { after }); // 描述与R8不可分开采用。
                } // 后续统一作者修订驱动预览。
                EditorCommand::EnvironmentChanged { before, after } => {
                    draft.manifest.environment = if undoing {
                        before.clone() // 一次撤销完整恢复路径、亮度与方向。
                    } else {
                        after.clone() // 重做采用同一份已验证完整快照。
                    };
                }
                EditorCommand::BoundaryPathChanged { index, before, after } => {
                    let document = draft.boundaries.as_mut().ok_or("空气墙文档已失效")?; // 不重建错误场景的墙。
                    let (previous, next) = if undoing { (after, before) } else { (before, after) }; // 对称恢复单对象。
                    crate::authoring::boundary::path_history::apply_path_version(document, *index, previous.as_ref(), next.as_ref())?;
                    draft.boundaries_dirty = true; // 同一水准的保存与实时碰撞修订。
                }
            }
            Ok(())
        })()
    };
    if let Err(error) = apply_result {
        deletion_preparation.reject_prepared(prepared_deletions.take());
        let command = if let Some(id) = terrain_scope { EditorCommand::TerrainCanvas { id, command: Box::new(command) } }
            else if root_water_scope { EditorCommand::RootWater(Box::new(command)) } else { command };
        if undoing {
            session.undo.push(command); // 目标身份或资源失效时原样恢复栈顶，历史不能因失败静默消失。
        } else {
            session.redo.push(command); // 重做失败同样保留命令，修复外部状态后仍可再次尝试。
        }
        editor.status = format!(
            "{}未执行：{error}；历史记录已保留",
            if undoing { "撤销" } else { "重做" }
        );
        return;
    }
    // 只在整条历史成功采用后发布增删责任，失败回栈不能更换票据或污染正式显隐基线。
    if let Some(map) = session.draft.as_ref().map(|draft| draft.scene_id.clone()) {
        if let Some(prepared) = prepared_deletions.take() {
            crate::authoring::placement_deletion::apply_prepared(&mut commands, &mut session, &map, prepared);
            // 完整表O(1)发布；纯变换型批次没有增删责任，不触碰表或无故推进删除修订。
        }
        let mut publish = |id: &str, deleted: bool| {
            if deleted {
                crate::authoring::placement_deletion::mark_deleted(&mut commands, &mut session, &map, id);
            } else {
                crate::authoring::placement_deletion::restore_deleted(&mut commands, &mut session, &map, id);
            }
        };
        match &command {
            EditorCommand::PlacementAdded(record) => publish(&record.id, undoing),
            EditorCommand::PlacementRemoved { record, .. } => publish(&record.id, !undoing),
            EditorCommand::BuildingTerrainChanged(change) => {
                if let BuildingPlacementChange::Added { record, .. } = &change.placement {
                    publish(&record.id, undoing);
                }
            }
            _ => {}
        }
    }
    if let Some((ref record, bounds, visible, spawn_when_missing)) = placement_world_update
        && let Err(error) = sync_placement_record_world_with_terrain(
            &mut commands,      // 同帧恢复Transform或删除显隐。
            &asset_server,      // 缺根恢复时按Scene0来源补预览。
            &mut session,       // 维护补预览所有权。
            &root_index,        // 稳定ID定向当前活动根。
            &placement_roots,   // 复核场景、ID和活动门。
            &record,            // 历史采用后的完整权威记录。
            bounds,             // 当前场景正式范围。
            visible,            // 撤销恢复或重做隐藏。
            spawn_when_missing, // 删除不补根，恢复允许补根。
            &terrain_queries,
        )
    {
        editor.status = format!("作者历史已采用，但资产画面同步失败：{error}"); // 草稿仍是权威，保存/热接管可最终修复画面。
    }
    if let Some((scene_id, placement_id, surfaces)) = placement_override_update {
        let write_override = |commands: &mut Commands, entity: Entity| {
            let mut root_commands = commands.entity(entity); // 覆盖数组与非空marker在同一延迟命令批次变化。
            root_commands.insert(DynamicWaterMeshOverrides::new(surfaces.clone())); // 每个已采用匹配根取得独立小型作者值。
            if surfaces.is_empty() {
                root_commands.remove::<ScenePlacementWaterEvidence>(); // 撤销到空覆盖后普通根退出保存等待查询。
            } else {
                root_commands.insert(ScenePlacementWaterEvidence); // 重做到非空覆盖后根重新进入最多256项证据集合。
            }
        };
        let wrote_preview = session
            .placement_previews
            .get(&placement_id)
            .copied()
            .is_some_and(|entity| {
                if placement_roots.get(entity).is_ok_and(|(_, instance)| {
                    instance.scene_id() == &scene_id
                        && instance.is_active()
                        && instance.id() == placement_id
                }) {
                    write_override(&mut commands, entity); // 精确预览引用必须仍属于当前场景、同ID活动根。
                    true
                } else {
                    false // 陈旧预览引用继续回退到已经采用的正式根。
                }
            });
        if !wrote_preview {
            root_index.for_each_active_root(&scene_id, &placement_id, &placement_roots, |entity| {
                write_override(&mut commands, entity)
            }); // 候选与退役旧根不接收未保存撤销值，也不枚举整个场景。
        }
    }
    if let Some(record) = portal_preview_update {
        refresh_owned_portal_preview(
            &mut commands,   // 旧预览与新文字请求同批替换。
            &preview_assets, // 复用既有共享预览资产。
            &mut session,    // 只替换仍属于未保存作者会话的预览。
            &record,         // 撤销或重做后的完整记录提供最终名称。
        );
    }
    let terrain_changed = matches!(&command, EditorCommand::TerrainChanged(_));
    let terrain_objects_changed = matches!(&command, EditorCommand::TerrainObjectsChanged { .. } | EditorCommand::ClipmapStitchesChanged {..});
    let scene_objects_changed = matches!(&command, EditorCommand::SceneObjectsChanged(_));
    let building_terrain_changed = match &command {
        EditorCommand::BuildingTerrainChanged(change) => Some((
            matches!(&change.placement, BuildingPlacementChange::Added { .. }),
            !change.heights.is_empty(),
        )),
        _ => None,
    }; // 联合命令随后一次推进摆放结构、地形和全局作者修订。
    let surface_changed = matches!(
        &command,
        EditorCommand::SurfaceChanged { .. } // 完成当前地表作者事务的受检参数或值转换。
            | EditorCommand::SurfaceLayerAdded { .. } // 完成当前地表作者事务的受检参数或值转换。
            | EditorCommand::SurfaceAuthoringChanged(_) // 整层删除历史同样驱动地表预览与配对保存。
    );
    let vegetation_layer = match &command {
        EditorCommand::VegetationChanged { layer_id, .. } => Some(layer_id.clone()),
        _ => None,
    };
    let model_vegetation_changed = matches!(&command, EditorCommand::ModelVegetationChanged { .. });
    if matches!(&command, EditorCommand::CourtyardBatch(change) if change.model_vegetation.is_some()) {
        session.vegetation_revision = session.vegetation_revision.wrapping_add(1);
    }
    let vegetation_authoring_changed =
        matches!(&command, EditorCommand::VegetationAuthoringChanged(_))
            || matches!(&command, EditorCommand::VegetationPaintChanged(change) if !change.canvases.is_empty()); // 仅组元数据不改变植被编译身份。
    let cross_compiled=if let EditorCommand::VegetationPaintChanged(change)=&command {
        if let Some(state)=vegetation_cross.as_deref_mut() {state.refresh(change);}
        session.vegetation_cross_busy=!change.canvases.is_empty();
        !change.canvases.is_empty()
    } else {false};
    let water_stroke = match &command {
        EditorCommand::WaterStrokeSnapshot(change) => {
            Some((change.layer_id.clone(), change.terrain.is_some()))
        } // 整笔仅推进一次相关修订。
        EditorCommand::WaterStroke {
            layer_id,
            height_changes,
            ..
        } => Some((layer_id.clone(), !height_changes.is_empty())), // 撤销/重做按命令是否含GLB顶点差分精确换代地形。
        _ => None,
    };
    let water_layers_changed = matches!(&command, EditorCommand::WaterLayersChanged(_));
    let water_definition_changed = matches!(
        &command,
        EditorCommand::WaterLevelChanged { .. } | EditorCommand::WaterDefinitionsChanged { .. }
    );
    let environment_changed = matches!(&command, EditorCommand::EnvironmentChanged { .. });
    let boundaries_changed = matches!(&command, EditorCommand::BoundaryPathChanged { .. }); // 单墙操作同样推动专用修订。
    let portals_changed = matches!(
        &command,
        EditorCommand::PortalAdded(_)
            | EditorCommand::PortalRemoved { .. }
            | EditorCommand::PortalLabelChanged { .. }
    ); // 增删或改名都会刷新当前列表、目标列表和保存修订。
    let placement_structure_changed = match &command {
        EditorCommand::PlacementAdded(_) => true, // 撤销和重做都增删一个实例根。
        EditorCommand::PlacementRemoved { .. } => true, // 删除与恢复都改变实例根和场景树结构。
        EditorCommand::CourtyardBatch(_) => true, // 整组成员共享一次结构与作者修订换代。
        EditorCommand::PlacementChanged { before, after, .. } => {
            placement_structure_differs(before, after)
                || before.height_mode!=after.height_mode
                || before.terrain_binding.as_ref().map(|binding|&binding.terrain_id)!=after.terrain_binding.as_ref().map(|binding|&binding.terrain_id) // 承托归属变化也必须撤销旧跟随完成信用。
        }
        _ => false,
    };
    let command = if let Some(id) = terrain_scope { EditorCommand::TerrainCanvas { id, command: Box::new(command) } }
        else if root_water_scope { EditorCommand::RootWater(Box::new(command)) } else { command };
    if undoing {
        session.redo.push(command);
        editor.status = "已撤销上一项作者操作".to_owned();
    } else {
        if command_uses_bounded_water_history(&command) {
            session.push_redone_bounded_water_history(command);
        } else {
            session.undo.push(command);
        }
        editor.status = "已重做作者操作".to_owned();
    }
    if global_environment_changed {
        global_environment.revision = global_environment.revision.wrapping_add(1); // 撤销与重做均使迟到保存结果失效。
        global_environment.dirty = global_environment.draft != global_environment.saved; // 回到已保存基线时立即解除全局保存责任。
        session.revision = session.revision.wrapping_add(1); // 联合保存身份与实时预览同帧换代。
        editor.dirty = global_environment.dirty || session.has_scene_changes(); // 顶部脏状态汇总两份独立作者文件。
    } else if scene_objects_changed {
        session.mark_scene_objects_changed(&mut editor);
    } else if placement_structure_changed {
        session.mark_placement_structure_changed(&mut editor); // 命令已经完整采用后才发布新结构代次。
    } else if let Some((placement_structure_changed, terrain_changed)) = building_terrain_changed {
        session.mark_building_terrain_changed(
            &mut editor,
            placement_structure_changed,
            terrain_changed,
        ); // 复合命令不能被else-if拆成只推进其中一个领域。
    } else if portals_changed {
        session.mark_portals_changed(&mut editor); // 只换代传送双列表与统一保存修订。
    } else if terrain_changed {
        session.mark_terrain_changed(&mut editor);
    } else if terrain_objects_changed {
        session.terrain_revision = session.terrain_revision.wrapping_add(1);
        session.mark_changed(&mut editor); // 对象历史不把空容器标为旧GLB待重写。
    } else if surface_changed {
        session.mark_surface_changed(&mut editor);
    } else if let Some(layer_id) = vegetation_layer {
        session.mark_vegetation_changed(&mut editor, &layer_id);
    } else if model_vegetation_changed {
        crate::authoring::model_vegetation::mark_changed(&mut session, &mut editor);
    } else if vegetation_authoring_changed {
        session.mark_vegetation_authoring_changed(&mut editor); // 撤销/重做同样走single-flight编译与保存链路。
        if cross_compiled {
            session.vegetation_compiled_revision=session.vegetation_revision;
            session.vegetation_compiled_structure_revision=session.vegetation_structure_revision;
        }
    } else if let Some((layer_id, terrain_changed)) = water_stroke {
        session.mark_water_stroke_changed(
            &mut editor,     // 全局作者revision只增加一次。
            &layer_id,       // R8脏层保持原笔划稳定身份。
            terrain_changed, // 只有复合命令含GLB顶点差分才推进地形修订。
        );
    } else if water_layers_changed {
        session.mark_water_layers_changed(&mut editor); // 结构撤销同样刷新真实水面和全部派生遮罩。
        if let Some(count) = session
            .draft
            .as_ref()
            .and_then(|draft| draft.water.as_ref())
            .map(|water| water.layers.len())
        {
            editor.selected_water_layer = editor.selected_water_layer.min(count.saturating_sub(1));
        }
    } else if water_definition_changed {
        session.mark_water_definition_changed(&mut editor);
        if let Some(water) = session
            .draft
            .as_ref()
            .and_then(|draft| draft.water.as_ref())
        {
            editor.selected_water_river = editor
                .selected_water_river
                .min(water.rivers.len().saturating_sub(1)); // 结构撤销后选择不得越过恢复后的河流数组。
            editor.selected_waterfall = editor
                .selected_waterfall
                .min(water.waterfalls.len().saturating_sub(1)); // 同步钳制瀑布选择。
            editor.selected_water_control_point = None; // 完整快照换代后避免旧点索引误指另一稳定点。
        }
    } else if environment_changed {
        session.mark_environment_changed(&mut editor); // 离屏请求按新修订重新原子采用。
    } else if boundaries_changed {
        session.mark_boundaries_changed(&mut editor); // 松手或按钮完成后只重建一次完整静态碰撞。
    } else {
        session.mark_changed(&mut editor);
    }
    if let Some(updates) = courtyard_world_updates {
        crate::authoring::courtyards::queue_visual_sync(
            &mut courtyard_state, // 唯一队列受最多十六根每帧的消费预算约束。
            &session,             // 使用本次结构修订后的场景身份冻结迟到校验。
            updates
                .into_iter()
                .map(|update| (update.index, update.record, update.visible))
                .collect(), // 所有成员先入队，不在本帧逐根扫描或写ECS。
        );
    }
}

#[cfg(test)]
mod tests; // 复合水笔差分原子性回归独立维护，主撤销系统保持在单文件上限内。

/// 无窗口回归保留同一个Local任务状态，逐次非阻塞推进正式历史系统。
#[cfg(test)]
pub(crate) fn drive_undo_redo_to_completion(app: &mut App) {
    let system = app.world_mut().register_system(apply_undo_redo);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        app.world_mut().run_system(system).expect("真实历史系统应能运行");
        let editor = app.world().resource::<SceneEditorState>();
        if !editor.undo_requested && !editor.redo_requested { break; }
        assert!(std::time::Instant::now() < deadline, "历史准备应在有限时间内完成：{}", editor.status);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    app.world_mut().unregister_system(system).expect("测试驱动结束后回收系统");
}
