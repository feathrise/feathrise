//! 跨网格图片逐帧登记，全部成员就绪后一次采用；撤销也走相同完整发布边界。
use super::super::{EditorAuthoringSession, terrain_documents::TerrainCanvas};
use super::{resources::VegetationPaintState, systems, types::*, worker};
use crate::resources::SceneEditorState;
use bevy::{
    prelude::*,
    tasks::{AsyncComputeTaskPool, futures::check_ready},
};
use feathrise_scene_content::ActiveDynamicScene;
use feathrise_scene::{DynamicBladeDraftSnapshot, TerrainVegetationDrafts};
use std::sync::Arc;

/// 每帧只登记一张R8，完整组的资源指针在同一发布边界交换。
fn upload(prepared: &mut [PreparedCanvas], images: &mut Assets<Image>) -> bool {
    for canvas in prepared {
        if let Some((id, image)) = canvas.images.pop_front() {
            canvas.handles.insert(id, images.add(image));
            return false;
        }
    }
    true
}
fn snapshots(
    prepared: &mut [PreparedCanvas],
    revision: u64,
) -> Vec<(feathrise_scene_content::TerrainId, DynamicBladeDraftSnapshot)> {
    prepared
        .iter_mut()
        .map(|canvas| {
            (
                canvas.id.clone(),
                DynamicBladeDraftSnapshot {
                    source_revision: revision,
                    definition: canvas.definition.clone(),
                    sources: std::mem::take(&mut canvas.sources),
                    densities: std::mem::take(&mut canvas.handles),
                    nonzero_layers: canvas.nonzero.clone(),
                },
            )
        })
        .collect()
}

pub(super) fn publish(
    mut state: ResMut<VegetationPaintState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut editor: ResMut<SceneEditorState>,
    mut images: ResMut<Assets<Image>>,
    active: Res<ActiveDynamicScene>,
    mut outputs: TerrainVegetationDrafts,
) {
    if let Some(mut batch) = state.staging.take() {
        let current = session
            .draft
            .as_ref()
            .is_some_and(|d| d.scene_id == batch.transaction.scene_id)
            && session.revision == state.expected_revision
            && !state.cancelled;
        if !current {
            state.cancelled = true;
            state.ending = true;
            state.queue.clear();
            return;
        }
        let ready = upload(&mut batch.prepared, &mut images);
        // 未进入当前玩法场景时仍允许离线作者数据采用；实际进入后由保存/作者上下文加载。
        let in_game = active.scene_id.as_ref() == Some(&batch.transaction.scene_id);
        if !ready
            || (in_game
                && !outputs.ready(
                    &batch.transaction.scene_id,
                    batch.prepared.iter().map(|v| v.id.clone()),
                ))
        {
            state.staging = Some(batch);
            return;
        }
        if in_game {
            let snapshots = snapshots(
                &mut batch.prepared,
                session.vegetation_revision.wrapping_add(1),
            );
            outputs
                .publish(&batch.transaction.scene_id, snapshots)
                .ok()
                .expect("同系统内目标预检成功后不可失效");
        }
        systems::adopt(batch, &mut state, &mut session, &mut editor);
    }
    if let Some((map, revision, task)) = state.refresh_work.as_mut()
        && let Some(result) = check_ready(task)
    {
        let identity = (map.clone(), *revision);
        state.refresh_work = None;
        match result {
            Ok(values) => state.refresh_staging = Some((identity.0, identity.1, values)),
            Err(error) => {
                editor.status = format!("跨网格历史预览准备失败：{error}");
                state.refresh.clear();
            }
        }
    }
    if let Some((map, revision, mut prepared)) = state.refresh_staging.take() {
        if session.draft.as_ref().is_none_or(|d| d.scene_id != map) {
            state.refresh.clear();
            return;
        }
        if revision != session.vegetation_revision {
            return;
        } // dirty集合保留，重新准备最新完整版本。
        if !upload(&mut prepared, &mut images)
            || (active.scene_id.as_ref() == Some(&map)
                && !outputs.ready(&map, prepared.iter().map(|v| v.id.clone())))
        {
            state.refresh_staging = Some((map, revision, prepared));
            return;
        }
        if active.scene_id.as_ref() == Some(&map) {
            outputs
                .publish(&map, snapshots(&mut prepared, revision))
                .ok()
                .expect("目标已预检");
        }
        state.refresh.clear();
    }
    if !state.refresh.is_empty() && state.refresh_work.is_none() && state.refresh_staging.is_none()
    {
        let Some(draft) = session.draft.as_ref() else {
            state.refresh.clear();
            return;
        };
        let values = state
            .refresh
            .iter()
            .filter_map(|id| {
                let canvas = if draft
                    .terrain_documents
                    .active
                    .as_ref()
                    .is_some_and(|a| a.definition.id == *id)
                {
                    Arc::new(TerrainCanvas::snapshot(draft))
                } else {
                    draft.terrain_documents.values.get(id)?.clone()
                };
                Some((id.clone(), canvas))
            })
            .collect::<Vec<_>>();
        state.refresh_work = Some((
            draft.scene_id.clone(),
            session.vegetation_revision,
            AsyncComputeTaskPool::get().spawn(async move {
                values
                    .iter()
                    .map(|(id, canvas)| worker::prepare_images(id, canvas))
                    .collect()
            }),
        ));
    }
}
