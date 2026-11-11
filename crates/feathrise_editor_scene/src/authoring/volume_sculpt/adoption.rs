//! 作者候选在场景联合发布前复验，发布后同帧记录源与历史；避免跨帧输入夹在两次采用之间。
use super::lifecycle::scene_bounds;
use super::types::*;
use crate::{
    authoring::{EditorAuthoringSession, EditorCommand},
    resources::SceneEditorState,
};
use bevy::{prelude::*, tasks::AsyncComputeTaskPool};
use feathrise_scene::TerrainVolumePreviewRequest;
use std::sync::Arc;

pub(super) fn validate(
    mut commands: Commands,
    mut state: ResMut<VolumeAuthoringState>,
    session: Res<EditorAuthoringSession>,
    mut editor: ResMut<SceneEditorState>,
    mut request: ResMut<TerrainVolumePreviewRequest>,
    time: Res<Time<Real>>,
    mode: Option<Res<crate::viewport::modes::ViewportModeState>>,
    spatial: Option<Res<crate::viewport::spatial_brush::SpatialBrushState>>,
    failures: Query<&feathrise_scene::TerrainPatchColliderFailure>,
    uploads: Query<&feathrise_scene::TerrainMeshUpload>,
) {
    let Some(pending) = state.adoption.as_ref() else {
        return;
    };
    let source_matches = pending.revision == session.revision
        && pending.spatial.as_ref().is_none_or(|r| {
            super::spatial::request_active(
                &state,
                r,
                &editor,
                &session,
                mode.as_deref(),
                spatial.as_deref(),
            )
        })
        && session.draft.as_ref().is_some_and(|draft| {
            draft.placements.placements.get(pending.index) == pending.expected.as_ref()
        })
        && pending.history.as_ref().is_none_or(|(undoing, change)| {
            let stack = if *undoing {
                &session.undo
            } else {
                &session.redo
            };
            stack
                .last()
                .is_some_and(|command| same_change(command, change))
        });
    let streamed = pending.next.as_ref().is_some_and(|s| {
        s.record
            .terrain_patch
            .as_ref()
            .is_some_and(|p| p.streamed_volume)
    });
    let failure = failures
        .get(pending.root)
        .ok()
        .map(|f| f.detail().to_owned());
    if source_matches && failure.is_none() && streamed {
        editor.status = if let Ok(upload) = uploads.get(pending.root) {
            let (done, total) = upload.progress();
            format!("三维网格分块上传：{done}/{total}；保留旧地形，完成后共同采用")
        } else {
            "正在准备全部分块的地表材质与真实碰撞…".into()
        };
    }
    // 分块上传和碰撞烘焙可以按内容规模持续进行；取消依赖真实失败与来源换代。
    if !source_matches
        || failure.is_some()
        || !streamed && time.elapsed_secs_f64() - pending.started > 30.0
    {
        let old = state.adoption.take().unwrap();
        state.spatial_transaction = None;
        let was_adopted = request.adopted_root() == Some(old.root);
        request.target = old.previous;
        if old.next.is_some() && !was_adopted {
            commands.entity(old.root).try_despawn();
        }
        if let Some(bytes) = old.bytes {
            state.retired_files.push(VolumeSaveFile {
                placement_id: String::new(),
                path: String::new(),
                bytes,
            });
        }
        if let Some(stroke) = state.stroke.as_mut() {
            stroke.tail = None;
            stroke.seed = None;
            stroke.last = None;
            stroke.first_click_done = true;
            stroke.released = true;
        }
        let reason = failure.unwrap_or_else(|| "来源换代或碰撞等待超时".into());
        editor.cancel_close_request(&format!(
            "三维雕刻联合采用已取消：{reason}；保存和历史未发布"
        ));
        editor.save_requested = false;
    }
}

pub(super) fn acknowledge(
    mut state: ResMut<VolumeAuthoringState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut editor: ResMut<SceneEditorState>,
    request: Res<TerrainVolumePreviewRequest>,
    time: Res<Time<Real>>,
    mut stream: ResMut<feathrise_scene::TerrainVolumeStreamControl>,
    views: Query<(&Camera, &feathrise_scene::DynamicClipmapAuthoringView)>,
) {
    if state
        .adoption
        .as_ref()
        .is_some_and(|pending| request.is_adopted(pending.root, pending.next.is_none()))
    {
        let pending = state.adoption.take().unwrap();
        if let Err(error) = adopt(&mut state, &mut session, &mut editor, pending) {
            editor.status = error;
        } else {
            if let Some(stroke) = state.stroke.as_mut() {
                stroke.started = time.elapsed_secs_f64();
            }
            if let Some(snapshot) = state.current.as_ref().filter(|s| {
                matches!(
                    s.source.as_ref(),
                    feathrise_terrain::volume::sparse::VolumeSource::Paged(_)
                )
            }) {
                stream.draft = state
                    .scope
                    .clone()
                    .map(|map| (map, snapshot.record.clone()));
                if let feathrise_terrain::volume::sparse::VolumeSource::Paged(v) =
                    snapshot.source.as_ref()
                {
                    stream.focus_override = Some(
                        Vec3::from_array(v.focus) + Vec3::from_array(snapshot.record.translation),
                    );
                }
                stream.focus_anchor = views
                    .iter()
                    .find(|(c, _)| c.is_active)
                    .map(|(_, v)| Vec3::new(v.focus_xz.x, v.focus_y, v.focus_xz.y));
            } else {
                stream.draft = None;
                stream.focus_override = None;
                stream.focus_anchor = None;
            }
        }
    }
    finish_stroke(&mut state, &mut session);
}

fn adopt(
    state: &mut VolumeAuthoringState,
    session: &mut EditorAuthoringSession,
    editor: &mut SceneEditorState,
    pending: Adoption,
) -> Result<(), String> {
    if pending.revision != session.revision {
        return Err("三维候选作者修订已经失效".into());
    }
    if let Some((undoing, change)) = pending.history.as_ref() {
        let stack = if *undoing {
            &session.undo
        } else {
            &session.redo
        };
        if !stack.last().is_some_and(|c| same_change(c, change)) {
            return Err("三维撤销栈已经换代，候选零采用停止".into());
        }
    }
    let draft = session.draft.as_mut().ok_or("三维雕刻草稿已失效")?;
    if draft.placements.placements.get(pending.index) != pending.expected.as_ref() {
        return Err("三维雕刻目标记录已换代，无法采用历史".into());
    }
    let id = pending
        .next
        .as_ref()
        .map(|s| s.record.id.clone())
        .or_else(|| pending.expected.as_ref().map(|r| r.id.clone()))
        .ok_or("缺少三维区域ID")?;
    match &pending.next {
        Some(snapshot) if pending.index == draft.placements.placements.len() => {
            draft.placements.placements.push(snapshot.record.clone())
        }
        Some(snapshot) => draft.placements.placements[pending.index] = snapshot.record.clone(),
        None if pending.index + 1 == draft.placements.placements.len() => {
            draft.placements.placements.pop();
        }
        None => return Err("三维创建撤销须保持追加顺序；目标后仍有其他记录".into()),
    }
    let map = draft.scene_id.clone();
    let next_record = pending.next.as_ref().map(|s| s.record.clone());
    let mut retired = Vec::new();
    if let Some(pos) = state.files.iter().position(|f| f.placement_id == id) {
        retired.push(state.files.swap_remove(pos));
    }
    if let (Some(snapshot), Some(bytes)) = (&pending.next, pending.bytes) {
        state.files.push(VolumeSaveFile {
            placement_id: id.clone(),
            path: snapshot.record.asset_path.clone(),
            bytes,
        });
    }
    state.retired_files.extend(retired);
    if let Some(snapshot) = &pending.next {
        state.register_source(&snapshot.source);
        session.placement_previews.insert(id.clone(), pending.root);
    } else {
        session.placement_previews.remove(&id);
    }
    state.current = pending.next;
    state.current_index = pending.index;
    state.root = Some(pending.root);
    state.unsaved_region = Some(id.clone());
    session.mark_placement_structure_changed(editor);
    if let Some(stroke) = state.stroke.as_mut() {
        stroke.revision = session.revision;
        stroke.seed = None;
    }
    // 本域原位更新至多32条元数据，避免每个体积段重新分帧扫描8192个普通摆放。
    state.index.records.retain(|(_, r, _)| r.id != id);
    if let Some(record) = next_record {
        let hole = record.terrain_cutout(scene_bounds(session).unwrap())?;
        state.index.records.push((pending.index, record, hole));
    }
    state.index.identity = Some((map, session.placement_structure_revision));
    state.index.next = session.draft.as_ref().unwrap().placements.placements.len();
    state.index.ready = true;
    if let Some((undoing, change)) = pending.history {
        let stack = if undoing {
            &mut session.undo
        } else {
            &mut session.redo
        };
        if !stack.last().is_some_and(|c| same_change(c, &change)) {
            return Err("三维撤销栈已换代，拒绝移动历史游标".into());
        }
        let command = stack.pop().unwrap();
        if undoing {
            session.redo.push(command);
        } else {
            session.undo.push(command);
        }
        editor.status = if undoing {
            "已完整撤销三维雕刻"
        } else {
            "已完整重做三维雕刻"
        }
        .into();
    } else {
        record_stroke(state, session);
        editor.status = if pending.spatial.is_some() {
            "整条空间雕刻已采用，地形、洞口与碰撞一致；可一次撤销"
        } else {
            "三维地形、洞口与碰撞已共同采用；松手形成一条撤销记录"
        }
        .into();
    }
    Ok(())
}

pub(super) fn finish_stroke(
    state: &mut VolumeAuthoringState,
    session: &mut EditorAuthoringSession,
) {
    if state.work.is_some() || state.adoption.is_some() || state.spatial_transaction.is_some() {
        return;
    }
    if !state
        .stroke
        .as_ref()
        .is_some_and(|s| s.released && s.first_click_done && s.tail.is_none() && s.seed.is_none())
    {
        return;
    }
    let _ = session; // 首次采用已登记历史，松手仅收口，避免再追加第二条命令。
    state.stroke = None;
}
fn record_stroke(state: &mut VolumeAuthoringState, session: &mut EditorAuthoringSession) {
    let Some(stroke) = state.stroke.as_mut() else {
        return;
    };
    let Some(after) = state.current.clone() else {
        return;
    };
    if let Some(slot) = stroke.history_slot {
        if let Some(EditorCommand::VolumeSculptChanged(change)) = session.undo.get_mut(slot) {
            change.after = after; // 前值保持首次落笔前，末值仅随完整联合采用换代。
        }
    } else {
        stroke.history_slot = Some(session.undo.len());
        session
            .undo
            .push(EditorCommand::VolumeSculptChanged(Box::new(
                VolumeSculptChange {
                    scene_id: state.scope.clone().unwrap(),
                    index: stroke.index,
                    before: stroke.before.clone(),
                    after,
                },
            )));
        let redo = std::mem::take(&mut session.redo);
        if !redo.is_empty() {
            state.release = Some(AsyncComputeTaskPool::get().spawn(async move {
                drop(redo);
            }));
        }
    }
}
fn same_change(command: &EditorCommand, change: &VolumeSculptChange) -> bool {
    matches!(command, EditorCommand::VolumeSculptChanged(c) if c.scene_id == change.scene_id && c.index == change.index && Arc::ptr_eq(&c.after.source, &change.after.source))
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
