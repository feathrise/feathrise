//! 稀疏高度历史按对象与源版本定向路由；格点恢复继续在唯一后台槽完成。
use super::*;
use crate::authoring::sparse_terrain::types::{SparseBuildingHistoryRequest, SparseHistoryTarget};

pub(super) fn command_scope(command: &EditorCommand) -> Option<&feathrise_scene_content::TerrainId> {
    match command {
        EditorCommand::TerrainCanvas { id, .. } => Some(id),
        _ => None,
    }
}

#[cfg(test)]
#[path = "history/tests.rs"]
mod tests; // 真实作者源验证异对象拒绝、同对象源换代和完整包装回栈。

pub(super) fn capture_target(
    command: &EditorCommand,
    state: &SparseTerrainAuthoringState,
) -> SparseHistoryTarget {
    SparseHistoryTarget {
        terrain: command_scope(command).cloned(),
        source: state.desired_source.clone(),
    }
}

pub(super) fn target_is_current(
    target: &SparseHistoryTarget,
    state: &SparseTerrainAuthoringState,
    session: &EditorAuthoringSession,
) -> bool {
    let Some(draft) = session.draft.as_ref() else {
        return false;
    };
    if state.desired_scene_id.as_ref() != Some(&draft.scene_id)
        || state.desired_source != target.source
        || draft.terrain_manifest().terrain.kind != SceneTerrainKind::SparseClipmap
    {
        return false;
    }
    if draft.manifest.terrains.is_none() {
        return target.source.is_none()
            && draft
                .terrain_documents
                .active
                .as_ref()
                .is_none_or(|active| active.definition.id == feathrise_scene_content::TerrainId::legacy())
            && target
                .terrain
                .as_ref()
                .is_none_or(|id| *id == feathrise_scene_content::TerrainId::legacy());
    }
    let Some(active) = draft.terrain_documents.active.as_ref() else {
        return false;
    };
    if draft.terrain_documents.loading || target.terrain.as_ref() != Some(&active.definition.id) {
        return false;
    }
    let Some(path) = draft.terrain_manifest().terrain.height_path.as_ref() else {
        return false;
    };
    // 显式对象的清单路径还携带作者命名空间；同 SceneId 和票据不足以证明它是同一源。
    feathrise_scene_content::sparse_height_manifest_terrain_id(&draft.scene_id, path)
        .is_ok_and(|id| target.source.as_ref() == Some(&(id, path.clone())))
}

/// 在既有撤销系统之前截获稀疏命令，并把块 IO 与差分计算放入后台单飞槽。
pub(crate) fn apply_sparse_terrain_undo_redo(
    mut state: ResMut<SparseTerrainAuthoringState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut editor: ResMut<SceneEditorState>,
    surface: Option<feathrise_scene::TerrainObjectClipmapGround>,
    live: Option<Res<SparseHeightLiveOverride>>,
) {
    if !editor.undo_requested
        && !editor.redo_requested
        && let Some(undoing) = state.deferred_history_undoing.take()
    {
        editor.undo_requested = undoing;
        editor.redo_requested = !undoing; // 笔划已收口后把原按钮请求交还统一历史链。
    }
    if state.stroke.active && (editor.undo_requested || editor.redo_requested) {
        state.deferred_history_undoing = Some(editor.undo_requested);
        editor.undo_requested = false;
        editor.redo_requested = false; // 防止下一既有系统越过尚未形成命令的当前笔划。
        editor.status = "当前稀疏地形笔划正在后台收口；完成后自动继续撤销/重做".into();
        return;
    }
    if session.saving {
        return; // 既有保存语义会拒绝保存期间的新历史输入。
    }
    crate::authoring::terrain_documents::scope_history(&mut session);
    let (undoing, candidate) = if editor.undo_requested {
        (true, session.undo.last())
    } else if editor.redo_requested {
        (false, session.redo.last())
    } else {
        return;
    };
    let Some(candidate) = candidate else {
        return;
    };
    if !matches!(
        candidate.unscoped(),
        EditorCommand::SparseTerrainChanged(_) | EditorCommand::BuildingSparseTerrainChanged(_)
    ) {
        return;
    }
    let target = capture_target(candidate, &state);
    if !target_is_current(&target, &state, &session) {
        // 下一统一历史系统会 request_focus 到 wrapper.id；这里不能抢走请求写入当前对象。
        return;
    }
    if let EditorCommand::BuildingSparseTerrainChanged(change) = candidate.unscoped() {
        if matches!(&change.placement, crate::authoring::BuildingPlacementChange::Added { .. })
            && session.placement_deletions.write_shared()
        {
            editor.status = "摆放删除表正在后台准备；完成后继续建筑联合撤销/重做".into();
            return; // 不消费按钮请求或栈顶，尚未开始的高度任务也不能越过删除表读侧。
        }
        if state.task.is_some()
            || state.author.is_none()
            || state.external_lease
            || state.building_history_ready.is_some()
        {
            editor.status = "稀疏作者任务尚未结束，完成后继续建筑联合撤销/重做".into();
            return;
        }
        let Some(snapshot) = surface.as_ref().zip(live.as_deref())
            .and_then(|(surface, live)| state.current_height_snapshot(&session, live, surface))
        else {
            editor.status = "正在等待当前作者高度正式采用；随后继续建筑联合撤销/重做".into();
            return; // 缺少完整内容身份时不能启动历史，更不能把None当作同代证明。
        };
        let request = SparseBuildingHistoryRequest {
            undoing,
            target,
            change: change.clone(),
            author_revision: session.revision,
            terrain_revision: session.terrain_revision,
            structure_revision: session.placement_structure_revision,
            ground_content: Some(snapshot.content_revision_words()),
        };
        building_history::start(&mut state, request);
        editor.undo_requested = false;
        editor.redo_requested = false;
        session.saving = true; // 原历史在完成前保留；所有格点计算只在后台候选执行。
        editor.status = "正在后台恢复建筑与稀疏平台的完整联合事务".into();
        return;
    }
    let EditorCommand::SparseTerrainChanged(changes) = candidate.unscoped() else {
        return; // 非稀疏命令继续交给既有历史系统。
    };
    if state.task.is_some()
        || state.author.is_none()
        || state.external_lease
        || state.building_history_ready.is_some()
    {
        editor.status = "稀疏地形作者任务正在完成；随后自动继续撤销/重做".into();
        return; // 请求保持为 true，下一帧重试同一栈顶。
    }
    let changes = changes.clone();
    if undoing {
        editor.undo_requested = false;
    } else {
        editor.redo_requested = false;
    }
    start_replay_task(
        &mut state,
        changes,
        undoing,
        target,
        session.revision,
        session.terrain_revision,
    );
    session.saving = true; // 任务完成前禁止其他命令改变当前栈顶或作者修订。
    editor.status = if undoing {
        "正在后台撤销稀疏地形笔划".into()
    } else {
        "正在后台重做稀疏地形笔划".into()
    };
}

fn start_replay_task(
    state: &mut SparseTerrainAuthoringState,
    changes: Arc<Vec<SparseTerrainHeightChange>>, // 主线程只共享已有完整差分，格点检查仍在后台。
    undoing: bool,
    target: SparseHistoryTarget,
    author_revision: u64,
    terrain_revision: u64,
) {
    let Some(author) = state.author.take() else {
        return;
    };
    let ticket = state.ticket;
    let scene_id = author.scene_id().clone();
    state.task = Some(IoTaskPool::get().spawn(async move {
        let mut author = author;
        if let Err(error) = author
            .verify_source_version()
            .and_then(|()| author.prefetch_change_tiles(&changes))
        {
            return SparseTerrainTaskOutput {
                ticket,
                scene_id,
                author: Some(author),
                context: SparseTerrainTaskContext::Replay {
                    undoing,
                    changes,
                    target,
                    author_revision,
                    terrain_revision,
                },
                result: Err(error),
            };
        }
        let compute_changes = changes.clone();
        let (author, result) = AsyncComputeTaskPool::get()
            .spawn(async move {
                let mut author = author;
                let result = author
                    .prepare_history_changes_loaded(compute_changes, !undoing)
                    .and_then(|edit| author.install_prepared_edit(edit))
                    .map(|applied| SparseTerrainTaskSuccess::Replayed {
                        tiles: applied.live_tiles,
                        rollback: applied.rollback,
                    });
                (author, result)
            })
            .await;
        SparseTerrainTaskOutput {
            ticket,
            scene_id,
            author: Some(author),
            context: SparseTerrainTaskContext::Replay {
                undoing,
                changes,
                target,
                author_revision,
                terrain_revision,
            },
            result,
        }
    }));
}

pub(super) fn rollback_candidate(
    state: &mut SparseTerrainAuthoringState,
    mut author: SparseTerrainAuthorSession,
    rollback: feathrise_scene_content::SparseTerrainEditRollback,
    rejected_tiles: Vec<Arc<SparseHeightTile>>,
) {
    let ticket = state.ticket;
    let scene_id = author.scene_id().clone();
    state.task = Some(AsyncComputeTaskPool::get().spawn(async move {
        // 安装和回滚持有同一个私有作者，旧脏集合与修订也随旧页精确恢复。
        let result = author
            .rollback_prepared_edit(rollback)
            .map(|()| SparseTerrainTaskSuccess::ReplayRolledBack);
        drop(rejected_tiles);
        SparseTerrainTaskOutput {
            ticket,
            scene_id,
            author: Some(author),
            context: SparseTerrainTaskContext::ReplayRollback,
            result,
        }
    }));
}

pub(super) fn retire_rollback(
    state: &mut SparseTerrainAuthoringState,
    rollback: feathrise_scene_content::SparseTerrainEditRollback,
) {
    let ticket = state.ticket;
    let scene_id = state
        .desired_scene_id
        .clone()
        .expect("已采用历史仍属于当前场景");
    state.task = Some(AsyncComputeTaskPool::get().spawn(async move {
        drop(rollback); // 长笔划可能保留大量原页；不能在完成帧逐页释放。
        SparseTerrainTaskOutput {
            ticket,
            scene_id,
            author: None,
            context: SparseTerrainTaskContext::RetireReplay,
            result: Ok(SparseTerrainTaskSuccess::ReplayRetired),
        }
    }));
}

pub(super) fn retire_output(
    state: &mut SparseTerrainAuthoringState,
    output: SparseTerrainTaskOutput,
) {
    let ticket = state.ticket;
    let scene_id = output.scene_id.clone();
    state.task = Some(AsyncComputeTaskPool::get().spawn(async move {
        drop(output);
        SparseTerrainTaskOutput {
            ticket,
            scene_id,
            author: None,
            context: SparseTerrainTaskContext::RetireReplay,
            result: Ok(SparseTerrainTaskSuccess::ReplayRetired),
        }
    }));
}
