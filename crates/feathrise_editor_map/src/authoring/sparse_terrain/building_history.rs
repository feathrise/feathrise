//! 建筑稀疏联合历史：后台保留原作者源，完整页候选与建筑一起原子恢复。
use super::*;
use crate::authoring::sparse_terrain::types::SparseBuildingHistoryRequest;
use crate::ui::placement_water::PlacementInstanceIndex;
use feathrise_scene::MapPlacementInstance;

pub(super) fn start(
    state: &mut SparseTerrainAuthoringState,
    request: SparseBuildingHistoryRequest,
) {
    let mut author = state.author.take().expect("调用方已证明唯一作者源空闲");
    let ticket = state.ticket;
    let map_id = author.map_id().clone();
    state.task = Some(IoTaskPool::get().spawn(async move {
        let changes = request.change.heights.clone(); // Arc浅复制，输入帧不遍历格点。
        let result = match author
            .verify_source_version()
            .and_then(|()| author.prefetch_prepared_change_tiles(&changes))
        {
            Err(error) => Err(error),
            Ok(_) => {
                let redo = !request.undoing;
                let output = AsyncComputeTaskPool::get()
                    .spawn(async move {
                        let result = author
                            .prepare_changes_loaded(changes, redo)
                            .map(SparseTerrainTaskSuccess::BuildingReplayed);
                        (author, result)
                    })
                    .await;
                author = output.0;
                output.1
            }
        };
        SparseTerrainTaskOutput {
            ticket,
            map_id,
            author: Some(author),
            context: SparseTerrainTaskContext::BuildingReplay(request),
            result,
        }
    }));
}

/// 作者源已归还，栈顶、作者与高度内容再次同代时，建筑与有限页一起恢复。
pub(crate) fn poll_sparse_building_history(
    mut commands: Commands,
    assets: Res<AssetServer>,
    roots: Query<(Entity, &MapPlacementInstance)>,
    root_index: Res<PlacementInstanceIndex>,
    mut sparse: ResMut<SparseTerrainAuthoringState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut editor: ResMut<MapEditorState>,
    mut live: ResMut<SparseHeightLiveOverride>,
    surface: Option<feathrise_scene::TerrainObjectClipmapGround>,
) {
    let Some((request, result)) = sparse.building_history_ready.take() else {
        return;
    };
    let top = if request.undoing {
        session.undo.last()
    } else {
        session.redo.last()
    };
    let matches_top = top.is_some_and(|command| {
        history::command_scope(command) == request.target.terrain.as_ref()
            && matches!(command.unscoped(), EditorCommand::BuildingSparseTerrainChanged(current)
                if std::sync::Arc::ptr_eq(&current.heights, &request.change.heights)
                    && placement_matches(&current.placement, &request.change.placement))
    });
    if !matches_top
        || !history::target_is_current(&request.target, &sparse, &session)
        || !editor.open
        || editor.close_requested
        || editor.new_map_dialog_open
        || session.revision != request.author_revision
        || session.terrain_revision != request.terrain_revision
        || session.placement_structure_revision != request.structure_revision
        || session
            .draft
            .as_ref()
            .is_none_or(|draft| editor.selected_map_id.as_ref() != Some(&draft.map_id))
    {
        session.saving = false;
        editor.status = "建筑联合历史结果已过期，建筑、地形与栈顶均未修改".into();
        return;
    }
    if let Some(expected) = request.ground_content {
        let Some(snapshot) = surface.as_ref()
            .and_then(|surface| sparse.current_height_snapshot(&session, &live, surface)) else {
            sparse.building_history_ready = Some((request, result));
            return; // 几何会话换代不伪造高度内容；同源完整页仍须与当前live逐项匹配。
        };
        // 稀疏历史冻结的是所选对象Clipmap内容，不能与建筑组合地面的全局几何身份混用。
        if snapshot.content_revision_words() != expected {
            session.saving = false;
            editor.status = "建筑校验期间地图或地形来源已换代".into();
            return;
        }
    } // 无来源约束的内部事务沿用原合同；正式撤销入口只生成Some完整内容身份。
    if matches!(&request.change.placement, crate::authoring::BuildingPlacementChange::Added { .. })
        && session.placement_deletions.write_shared()
    {
        sparse.building_history_ready = Some((request, result));
        editor.status = "摆放删除表正在后台读取；完成后共同恢复建筑与稀疏平台".into();
        return; // 原栈顶、完整页候选和saving串行门均保留，下帧重新核对全部身份。
    }
    let applied = result.and_then(|edit| {
        crate::authoring::buildings::apply_sparse_building_transaction(
            &mut commands,
            &assets,
            &root_index,
            &roots,
            &mut editor,
            &mut session,
            &mut sparse,
            &mut live,
            &request.change.placement,
            edit,
            request.undoing,
        )
    });
    session.saving = false;
    match applied {
        Err(error) => {
            editor.status = format!("建筑联合历史恢复失败：{error}；已保留原栈顶与作者源")
        }
        Ok(_) => {
            let command = if request.undoing {
                session.pop_undo().unwrap()
            } else {
                session.redo.pop().unwrap()
            };
            if request.undoing {
                session.redo.push(command);
            } else {
                session.undo.push(command);
            }
            editor.status = if request.undoing {
                "已共同撤销建筑与稀疏平台"
            } else {
                "已共同重做建筑与稀疏平台"
            }
            .into();
        }
    }
}

fn placement_matches(
    left: &crate::authoring::BuildingPlacementChange,
    right: &crate::authoring::BuildingPlacementChange,
) -> bool {
    use crate::authoring::BuildingPlacementChange::*;
    match (left, right) {
        (
            Added {
                record_index: a,
                record: x,
            },
            Added {
                record_index: b,
                record: y,
            },
        ) => a == b && x == y,
        (
            Changed {
                record_index: a,
                before: x,
                after: y,
            },
            Changed {
                record_index: b,
                before: z,
                after: w,
            },
        ) => a == b && x == z && y == w,
        _ => false,
    }
}

#[cfg(test)]
#[path = "building_history/tests.rs"]
mod tests; // 真实作者页与最小ECS联合验证栈顶、世界预览、换图取消和保存门。
