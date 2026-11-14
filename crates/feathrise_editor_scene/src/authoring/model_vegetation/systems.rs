//! 模型选择、根画布交接、真实视口输入与完整后台结果的身份采用。

use super::resources::*;
use crate::{
    authoring::{EditorAuthoringSession, EditorCommand, terrain_documents},
    components::{SceneEditorCamera, SceneEditorViewport},
    resources::{SceneEditorState, SceneEditorViewportState},
    types::EditorTool,
    ui::{gltf_browser::EditorGltfBrowserState, terrain_objects::TerrainObjectsEditorState},
};
use bevy::{
    prelude::*,
    tasks::{AsyncComputeTaskPool, IoTaskPool, futures::check_ready},
    ui::RelativeCursorPosition,
    world_serialization::WorldAsset,
};
use feathrise_scene::paint_surface::{PaintSurfaceSnapshot, read_model_surface_source};
use feathrise_scene_content::{ModelSurfaceVegetation, SceneInstancePath, ScenePlacementRecord, SurfaceTarget};
use std::{collections::VecDeque, sync::Arc};

pub(super) fn selected<'a>(
    session: &'a EditorAuthoringSession,
    browser: &EditorGltfBrowserState,
) -> Option<(usize, &'a ScenePlacementRecord)> {
    let id = browser.selected_placement_id.as_deref()?;
    let index = browser.placement_record_index(id, session.placement_structure_revision)?;
    let record = session.draft.as_ref()?.placements.placements.get(index)?;
    (record.id == id && record.terrain_patch.is_none()).then_some((index, record))
}

fn source(
    session: &EditorAuthoringSession,
    index: usize,
    record: &ScenePlacementRecord,
    epoch: u64,
) -> Option<Source> {
    Some(Source {
        scene: session.draft.as_ref()?.scene_id.clone(),
        id: record.id.clone(),
        index,
        path: record.asset_path.clone(),
        target: record.asset_target,
        epoch,
    })
}

fn record<'a>(
    session: &'a EditorAuthoringSession,
    source: &Source,
) -> Option<&'a ScenePlacementRecord> {
    let draft = session
        .draft
        .as_ref()
        .filter(|draft| draft.scene_id == source.scene)?;
    draft
        .placements
        .placements
        .get(source.index)
        .filter(|record| {
            record.id == source.id
                && record.asset_path == source.path
                && record.asset_target == source.target
                && record.terrain_patch.is_none()
                && record.editor.selectable()
        })
}

fn matrix(record: &ScenePlacementRecord) -> Mat4 {
    Mat4::from_scale_rotation_translation(
        Vec3::from_array(record.scale),
        Quat::from_rotation_y(record.yaw_degrees.to_radians()),
        Vec3::from_array(record.translation),
    )
}

/// GLB强句柄负责正常热更新通知；文件读取及拓扑索引仍只在后台重新建立一次。
pub(super) fn hot_reload(
    mut events: MessageReader<AssetEvent<WorldAsset>>,
    mut state: ResMut<ModelVegetationState>,
) {
    let id = state.handle.as_ref().map(Handle::id);
    let changed = events.read().take(64).any(|event| matches!(event,
        AssetEvent::Modified { id: event } | AssetEvent::Removed { id: event } if Some(*event) == id));
    if !events.is_empty() {
        events.clear();
        state.epoch = state.epoch.wrapping_add(1);
    } else if changed {
        state.epoch = state.epoch.wrapping_add(1);
    }
}

/// 模型种植使用根图层；先经过相同的笔划/编译停放门，才清除独立地形选择。
pub(super) fn focus_root(
    mouse: Res<ButtonInput<MouseButton>>,
    mut editor: ResMut<SceneEditorState>,
    browser: Res<EditorGltfBrowserState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut selection: ResMut<TerrainObjectsEditorState>,
    mut task: ResMut<terrain_documents::TerrainDocumentTask>,
    mut target: ResMut<feathrise_scene::TerrainObjectAuthoringTarget>,
    idle: terrain_documents::CanvasIdle,
) {
    if !editor.open
        || editor.tool != EditorTool::Vegetation
        || selected(&session, &browser).is_none()
        || session.saving
        || mouse.pressed(MouseButton::Left)
        || idle.busy(&session)
        || session.sparse_terrain_dirty
    {
        return;
    }
    terrain_documents::scope_history(&mut session);
    let history_len = session.undo.len();
    let Some(draft) = session.draft.as_mut() else {
        return;
    };
    if draft.terrain_documents.loading || draft.terrain_documents.scoped_through < history_len {
        return;
    }
    let changed = terrain_documents::root_water::focus(draft);
    selection.selected = None;
    selection.selected_index = None;
    task.requested = None;
    editor.terrain_canvas_target = None;
    target.terrain = None;
    if changed {
        session.revision = session.revision.wrapping_add(1);
        session.surface_revision = session.surface_revision.wrapping_add(1);
        session.vegetation_revision = session.vegetation_revision.wrapping_add(1);
        session.vegetation_structure_revision =
            session.vegetation_structure_revision.wrapping_add(1);
        session.water_revision = session.water_revision.wrapping_add(1);
    }
}

fn ray(
    viewport: &SceneEditorViewportState,
    cursors: &Query<&RelativeCursorPosition, With<SceneEditorViewport>>,
    cameras: &Query<(&Camera, &Transform), With<SceneEditorCamera>>,
) -> Option<Ray3d> {
    let cursor = cursors
        .single()
        .ok()
        .filter(|cursor| cursor.cursor_over())?
        .normalized?;
    let pixel = (cursor + Vec2::splat(0.5)) * viewport.render_size.as_vec2();
    let (camera, transform) = cameras.get(viewport.camera).ok()?;
    camera
        .viewport_to_world(&GlobalTransform::from(*transform), pixel)
        .ok()
}

#[allow(clippy::too_many_arguments)]
pub(super) fn capture(
    mouse: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    time: Res<Time<Real>>,
    viewport: Option<Res<SceneEditorViewportState>>,
    cursors: Query<&RelativeCursorPosition, With<SceneEditorViewport>>,
    cameras: Query<(&Camera, &Transform), With<SceneEditorCamera>>,
    browser: Res<EditorGltfBrowserState>,
    mut editor: ResMut<SceneEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut state: ResMut<ModelVegetationState>,
    layers: Option<Res<crate::ui::vegetation_layers::VegetationBlankLayerAllocation>>,
) {
    state.input_frame = true;
    let closing = editor.save_requested
        || editor.close_requested
        || editor.undo_requested
        || editor.redo_requested;
    let control = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    let context = editor.open
        && editor.tool == EditorTool::Vegetation
        && !session.saving
        && !session.vegetation_cross_busy
        && layers.as_ref().is_none_or(|state| !state.busy())
        && !closing
        && !control
        && session.draft.as_ref().is_some_and(|draft| {
            draft.terrain_documents.active.is_none() && !draft.terrain_documents.loading
        });
    let current = selected(&session, &browser).filter(|(_, record)| record.editor.selectable());
    let current_source =
        current.and_then(|(index, record)| source(&session, index, record, state.epoch));
    let layer = session
        .draft
        .as_ref()
        .and_then(|draft| draft.vegetation.as_ref())
        .and_then(|vegetation| vegetation.authoring.as_ref())
        .and_then(|authoring| authoring.layers.get(editor.selected_vegetation_layer))
        .filter(|layer| layer.enabled && !layer.locked)
        .map(|layer| layer.id.clone());
    let current_ray = viewport
        .as_deref()
        .and_then(|viewport| ray(viewport, &cursors, &cameras));
    if let Some(stroke) = state.stroke.as_mut() {
        stroke.release |= !context
            || !mouse.pressed(MouseButton::Left)
            || current_source.as_ref() != Some(&stroke.source)
            || layer.as_deref() != Some(&stroke.layer);
    }
    if !context {
        return;
    }
    let Some((index, record)) = current else {
        return;
    };
    let Some(source) = current_source else {
        return;
    };
    let Some(layer) = layer else {
        return;
    };
    let Some(ray) = current_ray else {
        return;
    };
    if mouse.just_pressed(MouseButton::Left) && state.stroke.is_none() {
        state.stroke = Some(Stroke {
            source,
            matrix: matrix(record),
            layer,
            data_index: index,
            initialized: false,
            before_bytes: 0,
            after_bytes: 0,
            sampled: false,
            before: session
                .draft
                .as_ref()
                .unwrap()
                .vegetation
                .as_ref()
                .unwrap()
                .model_surfaces
                .clone(),
            after: None,
            queue: VecDeque::new(),
            release: false,
            last: None,
        });
    }
    let Some(stroke) = state.stroke.as_mut().filter(|stroke| !stroke.release) else {
        return;
    };
    if !mouse.pressed(MouseButton::Left) {
        return;
    }
    let starting = !stroke.sampled;
    let (amount, exposure) = super::super::paint::vegetation_paint_exposure(
        editor.brush_strength,
        time.delta_secs(),
        starting,
    );
    let sample = Sample {
        ray,
        radius: editor.brush_radius,
        amount,
        exposure,
        erase: keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight),
    };
    stroke.sampled = true;
    if let Some(previous) = stroke.queue.back_mut().filter(|previous| {
        previous.ray == sample.ray
            && previous.radius == sample.radius
            && previous.erase == sample.erase
    }) {
        previous.exposure += sample.exposure;
        previous.amount = previous.amount.max(sample.amount);
    } else if stroke.queue.len() < MAX_QUEUED_SAMPLES {
        stroke.queue.push_back(sample);
    } else {
        stroke.release = true;
        editor.status = "模型表面绘制（实验）已达到32段后台队列预算；正在完成已受理笔划，请松手后继续".into();
    }
    session.model_vegetation_busy = state.busy();
}

fn finish(
    state: &mut ModelVegetationState,
    session: &mut EditorAuthoringSession,
    editor: &mut SceneEditorState,
) {
    let Some(stroke) = state.stroke.take() else {
        return;
    };
    if let Some(after) = stroke.after
        && session
            .draft
            .as_ref()
            .is_some_and(|draft| draft.scene_id == stroke.source.scene)
    {
        session.push_bounded_water_history(EditorCommand::RootWater(Box::new(
            EditorCommand::ModelVegetationChanged {
                index: stroke.data_index,
                object_id: stroke.source.id,
                layer_id: stroke.layer,
                before: stroke.before,
                after,
                heap_bytes: stroke.before_bytes.saturating_add(stroke.after_bytes),
            },
        )));
        session.redo.clear();
        editor.status = "模型表面绘制（实验）笔划已完成；Ctrl+Z 可撤销整笔".into();
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn advance(
    mut state: ResMut<ModelVegetationState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut editor: ResMut<SceneEditorState>,
    browser: Res<EditorGltfBrowserState>,
    server: Res<AssetServer>,
    viewport: Option<Res<SceneEditorViewportState>>,
    cursors: Query<&RelativeCursorPosition, With<SceneEditorViewport>>,
    cameras: Query<(&Camera, &Transform), With<SceneEditorCamera>>,
) {
    let input_frame = std::mem::take(&mut state.input_frame);
    if let Some(stroke) = &mut state.stroke {
        stroke.release |= !input_frame
            || !editor.open
            || editor.tool != EditorTool::Vegetation
            || editor.save_requested
            || editor.close_requested
            || editor.undo_requested
            || editor.redo_requested;
    }
    if state.retirement.as_mut().and_then(check_ready).is_some() {
        state.retirement = None;
    }
    if state.retirement.is_some() {
        session.model_vegetation_busy = state.busy();
        return;
    }
    if let Some(result) = state
        .pending
        .as_mut()
        .and_then(|pending| check_ready(&mut pending.task))
    {
        let pending = state.pending.take().unwrap();
        let valid = state.source.as_ref() == Some(&pending.source)
            && state.epoch == pending.source.epoch
            && record(&session, &pending.source)
                .is_some_and(|record| !pending.painting || matrix(record) == pending.matrix);
        let mut retired = None;
        let mut previous = None;
        if valid {
            match result {
                Ok(Work::Geometry(geometry)) => {
                    state.geometry = Some(geometry);
                }
                Ok(Work::Hover(hit)) => {
                    // 几何加载与整体姿态独立，但光标命中必须对应本次冻结的对象矩阵。
                    if record(&session, &pending.source).is_some_and(|record| matrix(record) == pending.matrix) {
                        state.hit = hit;
                        if editor.open && editor.tool == EditorTool::Vegetation {
                            editor.brush_cursor_world = state.hit.as_ref().map(|hit| hit.position);
                        }
                    }
                }
                Ok(Work::Paint {
                    targets,
                    changed,
                    last,
                    before_bytes,
                    after_bytes,
                }) => {
                    let current = state
                        .stroke
                        .as_ref()
                        .is_some_and(|stroke| stroke.source == pending.source)
                        && session.vegetation_revision == pending.revision;
                    if current && changed {
                        let stroke = state.stroke.as_mut().unwrap();
                        let values = &mut session
                            .draft
                            .as_mut()
                            .unwrap()
                            .vegetation
                            .as_mut()
                            .unwrap()
                            .model_surfaces;
                        let unchanged = pending
                            .original
                            .as_ref()
                            .is_some_and(|before| Arc::ptr_eq(before, values));
                        if unchanged {
                            if stroke.after.is_none() {
                                stroke.before_bytes = before_bytes;
                            }
                            stroke.after_bytes = after_bytes;
                            previous = Some(std::mem::replace(values, targets.clone()));
                            stroke.after = Some(targets.clone());
                            stroke.last = last.clone();
                            let adopted_index = stroke.data_index;
                            super::mark_changed(&mut session, &mut editor);
                            state.data_index = Some(adopted_index);
                        } else {
                            stroke.queue.clear();
                            stroke.release = true;
                        }
                    } else if current {
                        state.stroke.as_mut().unwrap().last = last.clone();
                    } else if let Some(stroke) = &mut state.stroke {
                        stroke.queue.clear();
                        stroke.release = true;
                    }
                    retired = Some(Work::Paint {
                        targets,
                        changed,
                        last,
                        before_bytes,
                        after_bytes,
                    });
                }
                Err(error) => {
                    editor.status = format!("模型表面绘制（实验）未写入：{error}");
                    if let Some(stroke) = &mut state.stroke {
                        stroke.queue.clear();
                        stroke.release = true;
                    } else {
                        state.failed = true;
                    }
                }
            }
        } else {
            retired = result.ok();
            if let Some(stroke) = &mut state.stroke {
                stroke.queue.clear();
                stroke.release = true;
            }
        }
        // 旧稀疏样本和未采用BVH的最后一次释放也交给后台；退役仍占用同一单飞槽。
        state.retirement = Some(AsyncComputeTaskPool::get().spawn(async move {
            drop((pending, retired, previous));
        }));
    }
    if state.pending.is_some() || state.retirement.is_some() {
        session.model_vegetation_busy = state.busy();
        return;
    }
    if state
        .stroke
        .as_ref()
        .is_some_and(|stroke| stroke.release && stroke.queue.is_empty())
    {
        finish(&mut state, &mut session, &mut editor);
    }
    let desired = state
        .stroke
        .as_ref()
        .map(|stroke| stroke.source.clone())
        .or_else(|| {
            (editor.open && editor.tool == EditorTool::Vegetation)
                .then(|| selected(&session, &browser))
                .flatten()
                .and_then(|(index, record)| source(&session, index, record, state.epoch))
        });
    if state.source != desired {
        let old = state.geometry.take();
        state.handle = None;
        state.hit = None;
        state.source = desired.clone();
        state.failed = false;
        state.scan = 0;
        state.scan_done = false;
        state.data_index = None;
        state.retirement = Some(AsyncComputeTaskPool::get().spawn(async move {
            drop(old);
        }));
    }
    session.model_vegetation_busy = state.busy();
    if state.retirement.is_some() {
        return;
    }
    let Some(source) = desired else {
        return;
    };
    let Some(record) = record(&session, &source) else {
        if let Some(stroke) = &mut state.stroke {
            stroke.release = true;
            stroke.queue.clear();
        }
        return;
    };
    let pose = matrix(record);
    if state.geometry.is_none() && !state.failed {
        let input = record.clone();
        let root = super::super::workspace_asset_root();
        if !record.asset_target.is_builtin() {
            state.handle = Some(server.load(format!(
                "{}#Scene0",
                record.asset_path.split('#').next().unwrap_or_default()
            )));
        }
        state.pending = Some(Pending {
            source,
            matrix: pose,
            revision: session.vegetation_revision,
            original: None,
            painting: false,
            task: AsyncComputeTaskPool::get().spawn(async move {
                // 文件等待归IO池；解析与BVH建索引归计算池，同一作者任务仍占用唯一在途槽。
                let source = IoTaskPool::get().spawn(async move {
                    read_model_surface_source(&root, &input.asset_path, input.asset_target)
                }).await?;
                source.build().map(Work::Geometry)
            }),
        });
        return;
    }
    if state.failed {
        if let Some(stroke) = &mut state.stroke {
            stroke.release = true;
            stroke.queue.clear();
        }
        return;
    }
    if !state.scan_done
        || (state.stroke.is_none()
            && state.structure_revision != session.vegetation_structure_revision)
    {
        if state.structure_revision != session.vegetation_structure_revision {
            state.scan = 0;
            state.data_index = None;
            state.structure_revision = session.vegetation_structure_revision;
        }
        let Some(values) = session
            .draft
            .as_ref()
            .and_then(|draft| draft.vegetation.as_ref())
            .map(|asset| &asset.model_surfaces)
        else {
            return;
        };
        let end = (state.scan + 64).min(values.len());
        for index in state.scan..end {
            if super::target(&values[index], &source.id) {
                state.data_index = Some(index);
                break;
            }
        }
        state.scan = end;
        state.scan_done = state.data_index.is_some() || end == values.len();
    }
    if !state.scan_done {
        return;
    }
    let geometry = state.geometry.clone();
    let data_index = state.data_index;
    if let Some(stroke) = state.stroke.as_mut() {
        if !stroke.initialized {
            let values = &session
                .draft
                .as_ref()
                .unwrap()
                .vegetation
                .as_ref()
                .unwrap()
                .model_surfaces;
            stroke.data_index = data_index.unwrap_or(values.len());
            stroke.initialized = true;
        }
        if let Some(geometry) = geometry
            && !stroke.queue.is_empty()
        {
            let targets = session
                .draft
                .as_ref()
                .unwrap()
                .vegetation
                .as_ref()
                .unwrap()
                .model_surfaces
                .clone();
            let original = Some(targets.clone());
            let data = targets.get(stroke.data_index).cloned().unwrap_or_else(|| {
                Arc::new(ModelSurfaceVegetation::empty(
                    &source.id,
                    geometry.topology(),
                    0.25,
                ))
            });
            let samples = stroke.queue.drain(..stroke.queue.len().min(4)).collect();
            let layer = stroke.layer.clone();
            let last = stroke.last.clone();
            let pose = stroke.matrix;
            let index = stroke.data_index;
            let target = SurfaceTarget::Model { object_id: source.id.clone() };
            let instance_path = SceneInstancePath::root(source.scene.clone());
            state.pending = Some(Pending {
                source,
                matrix: pose,
                revision: session.vegetation_revision,
                original,
                painting: true,
                task: AsyncComputeTaskPool::get().spawn(async move {
                    let surface = PaintSurfaceSnapshot::from_model(target, instance_path, geometry, pose)?;
                    super::worker::paint(
                        surface, data, &layer, samples, last, targets, index,
                    )
                }),
            });
        }
    } else if let Some(geometry) = geometry
        && input_frame
        && let Some(ray) = viewport
            .as_deref()
            .and_then(|viewport| ray(viewport, &cursors, &cameras))
    {
        let target = SurfaceTarget::Model { object_id: source.id.clone() };
        let instance_path = SceneInstancePath::root(source.scene.clone());
        state.pending = Some(Pending {
            source,
            matrix: pose,
            revision: session.vegetation_revision,
            original: None,
            painting: false,
            task: AsyncComputeTaskPool::get().spawn(async move {
                let surface = PaintSurfaceSnapshot::from_model(target, instance_path, geometry, pose)?;
                Ok(Work::Hover(surface.ray_surface(
                    ray.origin,
                    *ray.direction,
                    1_000_000.0,
                )))
            }),
        });
    }
    session.model_vegetation_busy = state.busy();
}
