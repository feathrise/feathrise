//! 单飞路径准备和来源核对；主帧不克隆整条控制点数组，也不计算曲线或刷印列表。
use super::types::*;
use crate::{
    authoring::{EditorAuthoringSession, volume_sculpt::VolumeAuthoringState},
    resources::{MapEditorNumericScrubMouseGuard, MapEditorState},
    types::EditorTool,
    viewport::{
        placement_move::MoveInputContext,
        spatial_brush::{
            resources::SpatialBrushState,
            types::{ApplyMode, Identity, SpatialBrushRequest},
        },
    },
};
use bevy::{
    prelude::*,
    tasks::{AsyncComputeTaskPool, IoTaskPool, block_on, futures_lite::future},
};
use feathrise_map_terrain::volume::{
    VolumeBrushKind, VolumePathParameters, VolumePathPlan, sparse::SPARSE_CELL_SIZE,
};
use std::sync::{Arc, atomic::AtomicBool};

fn poll<T>(task: &mut bevy::tasks::Task<T>) -> Option<T> {
    block_on(future::poll_once(task))
}

pub(super) fn build(
    nodes: Arc<[[f32; 3]]>,
    selected: Option<usize>,
    edit: Edit,
    parameters: VolumePathParameters,
    bounds: Rect,
    source_to_world: Mat4,
) -> BuiltPath {
    let mut next = nodes.to_vec(); // 完整复制、曲线和分配只在当前后台工作槽执行。
    let mut selected = selected;
    match edit {
        Edit::Refresh => {}
        Edit::Add(p) => {
            next.push(p.to_array());
            selected = None;
        }
        Edit::Delete(i) if i < next.len() => {
            next.remove(i);
            selected = (!next.is_empty()).then(|| i.min(next.len() - 1));
        }
        Edit::Replace(i, p) if i < next.len() => next[i] = p.to_array(),
        Edit::Load(library, i) => {
            if let Some(entry) = library.paths.get(i) {
                next.clone_from(&entry.nodes);
                selected = (!next.is_empty()).then_some(0);
            }
        }
        _ => {}
    }
    let plan = if next.len() >= 2 {
        VolumePathPlan::new_in_source_frame(&next, parameters, SPARSE_CELL_SIZE, bounds, source_to_world)
            .map(|p| Some(Arc::new(p)))
    } else {
        Ok(None)
    };
    let preview = plan
        .as_ref()
        .ok()
        .and_then(Option::as_ref)
        .map_or_else(Arc::default, |p| p.preview(49).into());
    BuiltPath {
        nodes: next.into(),
        selected,
        plan,
        preview,
    }
}

pub(in crate::viewport::spatial_brush) fn drive(
    mut state: ResMut<SpatialBrushState>,
    mut editor: ResMut<MapEditorState>,
    session: Res<EditorAuthoringSession>,
    volume: Res<VolumeAuthoringState>,
    guard: Res<MapEditorNumericScrubMouseGuard>,
    context: MoveInputContext,
) {
    state.sync_scope(&editor, &session);
    // 输入先推进意图代次，再轮询完成结果；同帧新XYZ不能被迟到的旧预览覆盖。
    if state.enabled
        && state.mode == ApplyMode::Path
        && state.path.selected.is_some()
        && state.path.observed_center != state.center
    {
        if let (Some(i), Some(center)) = (state.path.selected, state.center) {
            state.path.queue(Edit::Replace(i, center));
            state.path.observed_center = Some(center);
        }
    }
    if let Some(task) = state.path.retire.as_mut() {
        if poll(task).is_none() {
            return;
        }
        state.path.retire = None;
    }
    if let Some(result) = state.path.task.as_mut().and_then(poll) {
        state.path.task = None;
        if state.path.working_revision == state.path.revision {
            let old = (
                std::mem::replace(&mut state.path.nodes, result.nodes),
                state.path.plan.take(),
                std::mem::replace(&mut state.path.preview, result.preview),
            );
            state.path.selected = result.selected;
            match result.plan {
                Ok(plan) => {
                    state.path.plan = plan;
                    state.path.error = None;
                }
                Err(error) => {
                    editor.status = error.clone();
                    state.path.error = Some(error);
                }
            }
            state.path.applied_revision = state.path.revision;
            if let Some(i) = state.path.selected {
                state.center = state.path.nodes.get(i).copied().map(Vec3::from_array);
            }
            state.path.observed_center = state.center;
            state.path.release(old);
        } else {
            state.path.release(result);
        }
        return;
    }
    if let Some(result) = state.path.io.as_mut().and_then(poll) {
        state.path.io = None;
        let scope = state
            .scope
            .as_ref()
            .and_then(|_| session.draft.as_ref())
            .map(|draft| (draft.map_id.0.clone(), draft.folder.clone()));
        if scope == state.path.io_scope {
            match result.library {
                Ok(library) => {
                    if result.saved {
                        state.path.saved_index = library.paths.len().saturating_sub(1);
                        editor.status = "路径已保存，可重开加载；地形没有因保存路径改变".into();
                    }
                    let old = state.path.library.replace(library);
                    state.path.release(old);
                }
                Err(error) => editor.status = error,
            }
        } else {
            state.path.release(result);
        }
        return;
    }
    let active = state.enabled
        && state.mode == ApplyMode::Path
        && Identity::capture(&editor, &session).is_some();
    if !active
        || session.saving
        || volume.busy()
        || state.pending.is_some()
        || state.drag.is_some()
        || state.owns_mouse
        || guard.blocked()
        || !context.sculpt_parameters_available(&editor)
    {
        return;
    }
    let draft = session.draft.as_ref().unwrap();
    let (bounds, source_to_world) = source_frame(draft);
    if state.path.task.is_none() && state.path.edit.is_none()
        && state.path.plan.as_ref().is_some_and(|p| !p.matches_source(bounds, source_to_world))
    {
        state.path.queue(Edit::Refresh);
    }
    if state.path.task.is_none()
        && let Some(edit) = state.path.edit.take()
    {
        let nodes = state.path.nodes.clone();
        let parameters = state.path.parameters;
        let selected = state.path.selected;
        state.path.working_revision = state.path.revision;
        state.path.task = Some(
            AsyncComputeTaskPool::get()
                .spawn(async move { build(nodes, selected, edit, parameters, bounds, source_to_world) }),
        );
    }
    if state.path.io.is_none() && !state.path.library_attempted {
        let draft = session.draft.as_ref().unwrap();
        let folder = draft.folder.clone();
        let map = draft.map_id.0.clone();
        state.path.io_scope = Some((map.clone(), folder.clone()));
        state.path.io_active = Arc::new(AtomicBool::new(true));
        state.path.library_attempted = true;
        state.path.io = Some(IoTaskPool::get().spawn(async move {
            IoResult {
                library: crate::authoring::load_sculpt_paths(folder, map).await,
                saved: false,
            }
        }));
    }
}

fn source_frame(draft: &crate::authoring::EditorMapDraft) -> (Rect, Mat4) {
    let manifest = draft.terrain_manifest();
    (
        Rect::from_corners(Vec2::from_array(manifest.world_min), Vec2::from_array(manifest.world_max)),
        draft.terrain_documents.active.as_ref().map_or(Mat4::IDENTITY, |active|
            active.definition.transform.as_transform().to_matrix()),
    )
}

pub(in crate::viewport::spatial_brush) fn build_request(
    state: &SpatialBrushState,
    editor: &MapEditorState,
    session: &EditorAuthoringSession,
) -> Result<SpatialBrushRequest, String> {
    if !state.path.ready(state.center) {
        return Err(state
            .path
            .error
            .clone()
            .unwrap_or_else(|| "请设置至少两个不同节点，并等待完整路径预览就绪".into()));
    }
    let kind = match editor.tool {
        EditorTool::VolumeDig => VolumeBrushKind::Dig,
        EditorTool::VolumeFill => VolumeBrushKind::Fill,
        EditorTool::VolumeSmooth => VolumeBrushKind::Smooth,
        EditorTool::VolumeFlatten => VolumeBrushKind::Flatten,
        _ => return Err("当前工具不是三维雕刻".into()),
    };
    if !editor.brush_strength.is_finite() || !(0.01..=1.).contains(&editor.brush_strength) {
        return Err("路径雕刻强度无效".into());
    }
    let identity = Identity::capture(editor, session).ok_or("路径作者地图已失效")?;
    let path = state.path.plan.as_ref().unwrap().clone();
    let (bounds, source_to_world) = source_frame(session.draft.as_ref().unwrap());
    if !path.matches_source(bounds, source_to_world) {
        return Err("地形对象姿态或尺寸已变化，请等待路径预览重新准备".into());
    }
    let brush = feathrise_map_terrain::volume::VolumeBrush {
        kind, strength: editor.brush_strength, ..path.node_brush(0).unwrap()
    };
    Ok(SpatialBrushRequest {
        identity,
        brush,
        path: Some(path),
    })
}
