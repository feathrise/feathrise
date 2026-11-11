//! 非阻塞采用、整笔历史和有界索引；CPU输出绝不绕过联合场景完成门。
use super::{adoption, types::*};
use crate::{
    authoring::{EditorAuthoringSession, EditorCommand, WaterFileHandoff},
    components::EditorPlacedAsset,
    resources::SceneEditorState,
    ui::placement_water::PlacementInstanceIndex,
};
use bevy::{
    prelude::*,
    tasks::{AsyncComputeTaskPool, IoTaskPool, futures::check_ready},
};
use feathrise_scene::{
    DeferredTerrainPreviewCollider, ScenePlacementInstance,
    TerrainVolumePreviewRequest, TerrainVolumePreviewTarget, terrain_placement_collision,
};
use std::sync::Arc;

pub(super) fn poll(
    mut commands: Commands,
    mut state: ResMut<VolumeAuthoringState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut editor: ResMut<SceneEditorState>,
    mut request: ResMut<TerrainVolumePreviewRequest>,
    meshes: Option<Res<Assets<Mesh>>>,
    mut materials: Option<ResMut<Assets<StandardMaterial>>>,
    mut material: Local<Option<Handle<StandardMaterial>>>,
    ground: Option<feathrise_scene::TerrainObjectClipmapGround>,
    time: Res<Time<Real>>,
    mut stream: ResMut<feathrise_scene::TerrainVolumeStreamControl>,
    mode: Option<Res<crate::viewport::modes::ViewportModeState>>,
    spatial: Option<Res<crate::viewport::spatial_brush::SpatialBrushState>>,
    owner: Option<Res<feathrise_scene::TerrainObjectAuthoringTarget>>,
) {
    if state.release.as_mut().and_then(check_ready).is_some() {
        state.release = None;
    }
    let map = session.draft.as_ref().map(|d| d.scene_id.clone());
    if state.scope != map {
        state.scope = map.clone();
        state.stroke = None;
        state.spatial_transaction = None;
        state.current = None;
        state.root = None;
        state.unsaved_region = None;
        state.index = default();
        state.saved_sequence = session.successful_save_sequence;
        let files = std::mem::take(&mut state.files);
        state.retired_files.extend(files);
        if let Some(old) = state.adoption.take() {
            if old.next.is_some() && request.adopted_root() != Some(old.root) {
                commands.entity(old.root).try_despawn();
            }
            let file = old.bytes.map(|bytes| VolumeSaveFile {
                placement_id: String::new(),
                path: String::new(),
                bytes,
            });
            state.retired_files.extend(file);
        }
        request.target = None; // scene在同一场景身份门内恢复或退休旧支撑。
        stream.draft = None;
        stream.focus_override = None;
        stream.focus_anchor = None;
    }
    if state.saved_sequence != session.successful_save_sequence {
        state.saved_sequence = session.successful_save_sequence;
        state.unsaved_region = None; // 正式交接另有既有single-flight门，输入不能趁保存成功抢旧预览。
        state.current = None; // 下一笔重开正式摘要文件，不能用保存前的内存源掩盖外部文件变化。
        state.spatial_transaction = None;
        stream.draft = None;
        let files = std::mem::take(&mut state.files);
        state.retired_files.extend(files);
    }
    scan_records(&mut state, &session);
    if !stream.retirement_busy && state.work.as_mut().is_some_and(|w| w.task.is_finished()) {
        let mut work = state.work.take().unwrap();
        let result = check_ready(&mut work.task).expect("已完成任务只非阻塞取一次");
        let source_matches = work.opening
            || work.history.is_some()
            || work.source.as_ref().is_none_or(|source| {
                state
                    .current
                    .as_ref()
                    .is_some_and(|current| Arc::ptr_eq(&current.source, source))
            });
        let ground_matches = work.ground.is_none_or(|words| {
            ground
                .as_ref()
                .and_then(|g| g.snapshot.as_ref())
                .is_some_and(|g| g.scene_id == work.scene_id && g.content_revision_words() == words)
        });
        let spatial_matches = work.spatial.as_ref().is_none_or(|r| {
            super::spatial::request_active(
                &state,
                r,
                &editor,
                &session,
                mode.as_deref(),
                spatial.as_deref(),
            )
        });
        if map.as_ref() != Some(&work.scene_id)
            || session.revision != work.revision
            || !source_matches
            || !ground_matches
            || !spatial_matches
        {
            state.release = Some(AsyncComputeTaskPool::get().spawn(async move {
                drop(result);
                drop(work);
            }));
            state.stroke = None;
            state.spatial_transaction = None;
            editor.status = "三维雕刻来源已换代，迟到结果已取消；当前完整地形保持不变".into();
        } else {
            match result {
                Ok(JobOutput::Unchanged { at_boundary }) => {
                    state.spatial_transaction = None;
                    if let Some(stroke) = state.stroke.as_mut() {
                        stroke.started = time.elapsed_secs_f64();
                        stroke.seed = None; // 首次平地平滑无改动也须收口，不能每帧重复创建同一空候选。
                    }
                    if at_boundary {
                        // 保护带内没有新改动时保留本笔；移回内部后仍可继续，不能冒充采用失败。
                        editor.status =
                            "已触及本块三维区域的保护边界；移动笔刷可继续编辑区域内地形".into();
                    }
                }
                Ok(JobOutput::Staged(transaction)) => {
                    // 批次根只在作者域私有状态推进；整条路径完成前，正式源、预览和撤销栈不变。
                    editor.status = format!(
                        "空间雕刻处理中：{}/{}个刷印；整条路径完成后采用…",
                        transaction.batches.completed_stamps(),
                        transaction.batches.total_stamps(),
                    );
                    state.spatial_transaction = Some(*transaction);
                    if let Some(stroke) = state.stroke.as_mut() {
                        stroke.started = time.elapsed_secs_f64();
                    }
                }
                Ok(JobOutput::Opened(snapshot)) => {
                    state.register_source(&snapshot.source);
                    state.current = Some(snapshot);
                    state.current_index = work.index;
                    if work.spatial.is_some()
                        && let Some(stroke) = state.stroke.as_mut()
                    {
                        stroke.started = time.elapsed_secs_f64();
                    }
                }
                Ok(JobOutput::Prepared(prepared)) => {
                    if let Err(error) = prepared
                        .fields
                        .validate_storage_limit(stream.gpu_storage_limit)
                    {
                        state.release =
                            Some(AsyncComputeTaskPool::get().spawn(async move { drop(prepared) }));
                        state.stroke = None;
                        state.spatial_transaction = None;
                        editor.status = format!("三维雕刻未采用：{error}");
                        return;
                    }
                    let (Some(_), Some(materials)) = (meshes.as_deref(), materials.as_deref_mut())
                    else {
                        state.release = Some(AsyncComputeTaskPool::get().spawn(async move {
                            drop(prepared);
                        }));
                        state.stroke = None;
                        state.spatial_transaction = None;
                        editor.status = "当前宿主未安装三维网格/材质存储，雕刻结果未采用".into();
                        return;
                    };
                    let bounds = scene_bounds(&session).expect("上方同图草稿证明");
                    let snapshot = prepared.snapshot;
                    state.register_source(&snapshot.source); // 候选取消后主帧也不成为密度的最后释放者。
                    let render_record = prepared.render_record;
                    let hole = render_record
                        .terrain_cutout(bounds)
                        .expect("后台合法雕刻记录")
                        .expect("自动替换区域");
                    let material = material
                        .get_or_insert_with(|| {
                            materials.add(StandardMaterial {
                                base_color: Color::WHITE,
                                perceptual_roughness: 1.0,
                                ..default()
                            })
                        })
                        .clone();
                    let root = commands
                        .spawn((
                            EditorPlacedAsset,
                            DespawnOnExit(feathrise_scene_core::state::ScenePhase::Active), // 尚未取得场景联合候选信用的根也随离场完整回收。
                            ScenePlacementInstance::new(&work.scene_id, &render_record),
                            Transform::from_translation(Vec3::from_array(
                                snapshot.record.translation,
                            )),
                            Visibility::Hidden,
                            prepared.fields,
                            DeferredTerrainPreviewCollider,
                            feathrise_scene::TerrainVolumeAuthorPreviewRoot(feathrise_scene_content::SceneInstancePath::root(work.scene_id.clone()).terrain(
                                render_record.terrain_patch.as_ref().and_then(|patch| patch.terrain_id.clone())
                                    .unwrap_or_else(feathrise_scene_content::TerrainId::legacy))),
                            feathrise_scene::TerrainMeshUpload::new(prepared.meshes, material),
                        ))
                        .insert(terrain_placement_collision(&render_record).unwrap())
                        .id();
                    if let Some(window) = prepared.resident_window {
                        commands.entity(root).insert(window);
                    }
                    if render_record.terrain_patch.as_ref().is_some_and(|patch| patch.terrain_id.is_some())
                        && let Some(parent) = owner.as_ref().and_then(|owner| owner.instance()) {
                        commands.entity(root).insert(ChildOf(parent)); // 候选、碰撞和洞口使用同一地形源坐标，随对象整体移动。
                    }
                    state.adoption = Some(Adoption {
                        root,
                        revision: work.revision,
                        index: work.index,
                        expected: session
                            .draft
                            .as_ref()
                            .and_then(|d| d.placements.placements.get(work.index))
                            .cloned(),
                        next: Some(snapshot.clone()),
                        bytes: Some(prepared.bytes),
                        previous: request.target.clone(),
                        history: work.history,
                        started: time.elapsed_secs_f64(),
                        spatial: work.spatial,
                    });
                    request.target = Some(TerrainVolumePreviewTarget {
                        scene_id: work.scene_id,
                        entity: root,
                        record: render_record,
                        world_cutout: hole,
                        removal: false,
                    });
                    editor.status = "三维网格已生成，正在等待洞口与真实碰撞共同采用…".into();
                }
                Err(error) => {
                    state.spatial_transaction = None;
                    if let Some(stroke) = state.stroke.as_mut() {
                        stroke.tail = None;
                        stroke.seed = None;
                        stroke.last = None;
                        stroke.released = true;
                        stroke.first_click_done = true;
                    }
                    editor.status = format!("三维雕刻未采用：{error}");
                }
            }
        }
    }
    if state.work.is_none()
        && state.adoption.is_none()
        && state
            .stroke
            .as_ref()
            .is_some_and(|s| s.revision != session.revision)
    {
        state.stroke = None; // 通用作者动作的后写优先，不把别人的修订塞进本笔历史。
        state.spatial_transaction = None;
    }
    adoption::finish_stroke(&mut state, &mut session);
    release_idle(&mut state);
}

fn scan_records(state: &mut VolumeAuthoringState, session: &EditorAuthoringSession) {
    let Some(draft) = session.draft.as_ref() else {
        return;
    };
    let identity = (draft.scene_id.clone(), session.placement_structure_revision);
    if state.index.identity.as_ref() != Some(&identity) {
        state.index = RecordIndex {
            identity: Some(identity),
            ..default()
        };
    }
    if state.index.ready {
        return;
    }
    let end = (state.index.next + RECORD_SCAN).min(draft.placements.placements.len());
    let bounds = scene_bounds(session).unwrap();
    for (offset, record) in draft.placements.placements[state.index.next..end]
        .iter()
        .enumerate()
    {
        let target = draft.terrain_documents.active.as_ref().map(|active| &active.definition.id);
        if !record.terrain_patch.as_ref().is_some_and(|patch| patch.terrain_id.as_ref().map_or(
            target.is_none_or(|id| *id == feathrise_scene_content::TerrainId::legacy()), |id| Some(id) == target)) {
            continue;
        }
        if state.index.records.len() >= feathrise_scene_content::MAX_TERRAIN_CUTOUTS {
            state.index.error = Some("三维地形超过32个区域，作者索引拒绝扩大".into());
            state.index.ready = true;
            return;
        }
        match record.terrain_cutout(bounds) {
            Ok(hole) => state
                .index
                .records
                .push((state.index.next + offset, record.clone(), hole)),
            Err(error) => state.index.error = Some(error),
        }
    }
    state.index.next = end;
    state.index.ready = end == draft.placements.placements.len();
}
pub(super) fn scene_bounds(session: &EditorAuthoringSession) -> Option<Rect> {
    session.draft.as_ref().map(|d| {
        Rect::from_corners(
            Vec2::from_array(d.terrain_manifest().world_min),
            Vec2::from_array(d.terrain_manifest().world_max),
        )
    })
}

fn release_idle(state: &mut VolumeAuthoringState) {
    if state.work.is_some() || state.adoption.is_some() || state.release.is_some() {
        return;
    }
    let mut dead = Vec::new();
    let mut i = 0;
    while i < state.holds.len() {
        if Arc::strong_count(&state.holds[i]) == 1 {
            dead.push(state.holds.swap_remove(i));
        } else {
            i += 1;
        }
    }
    let files = std::mem::take(&mut state.retired_files);
    if !dead.is_empty() || !files.is_empty() {
        state.release = Some(AsyncComputeTaskPool::get().spawn(async move {
            drop(dead);
            drop(files);
        }));
    }
}
pub(super) fn history(
    mut state: ResMut<VolumeAuthoringState>,
    mut editor: ResMut<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    mut request: ResMut<TerrainVolumePreviewRequest>,
    index: Res<PlacementInstanceIndex>,
    roots: Query<(Entity, &ScenePlacementInstance)>,
    time: Res<Time<Real>>,
    handoff: Res<WaterFileHandoff>,
    stream: Res<feathrise_scene::TerrainVolumeStreamControl>,
    mut documents: Option<ResMut<super::super::terrain_documents::TerrainDocumentTask>>,
    mut terrain_selection: Option<ResMut<crate::ui::terrain_objects::TerrainObjectsEditorState>>,
) {
    if state.busy()
        || stream.native_busy
        || session.saving
        || !(editor.undo_requested || editor.redo_requested)
    {
        return;
    }
    let undoing = editor.undo_requested;
    let command = if undoing {
        session.undo.last()
    } else {
        session.redo.last()
    };
    let Some(EditorCommand::VolumeSculptChanged(change)) = command else {
        return;
    };
    let Some(draft) = session.draft.as_ref().filter(|d| d.scene_id == change.scene_id) else {
        return;
    };
    if draft.manifest.terrains.is_some() {
        let id = change.after.record.terrain_patch.as_ref().and_then(|patch| patch.terrain_id.clone())
            .unwrap_or_else(feathrise_scene_content::TerrainId::legacy);
        if draft.terrain_documents.active.as_ref().is_none_or(|active| active.definition.id != id) {
            if let (Some(documents), Some(selection)) = (documents.as_deref_mut(), terrain_selection.as_deref_mut()) {
                super::super::terrain_documents::request_focus(documents, selection, draft, &id);
            }
            return; // 先恢复历史所属的作者源，再进入原有密度恢复与联合采用门。
        }
    }
    if state
        .unsaved_region
        .as_ref()
        .is_some_and(|id| *id != change.after.record.id)
    {
        editor.undo_requested = false;
        editor.redo_requested = false;
        editor.status = "请先保存当前三维区域，再撤销另一块区域；历史记录仍保留".into();
        return;
    }
    if handoff.placement_handoff_in_flight_for(&draft.scene_id) {
        return;
    }
    let expected = if undoing {
        Some(&change.after.record)
    } else {
        change.before.as_ref().map(|s| &s.record)
    };
    if draft.placements.placements.get(change.index) != expected {
        editor.undo_requested = false;
        editor.redo_requested = false;
        editor.status = "三维历史与当前摆放记录不一致，历史未弹出".into();
        return;
    }
    let next = if undoing {
        change.before.clone()
    } else {
        Some(change.after.clone())
    };
    if let Some(snapshot) = next {
        if let Err(error) = state.reserve_restore() {
            editor.status = error;
            return;
        }
        let folder = draft.terrain_folder().to_owned();
        let physics = stream.physics_focus;
        let lods = stream.display_lods.clone();
        state.work = Some(Work {
            task: IoTaskPool::get().spawn(async move {
                super::streamed::restore(snapshot, folder, physics, lods).await
            }),
            scene_id: draft.scene_id.clone(),
            revision: session.revision,
            source: None,
            index: change.index,
            history: Some((undoing, change.clone())),
            ground: None,
            opening: false,
            spatial: None,
        });
    } else {
        let mut candidates = Vec::new();
        index.for_each_active_root(&draft.scene_id, &change.after.record.id, &roots, |root| {
            if roots
                .get(root)
                .is_ok_and(|(_, r)| r.matches_record(&draft.scene_id, &change.after.record, &[]))
            {
                candidates.push(root);
            }
        });
        let streamed = request.target.as_ref().filter(|t| {
            t.scene_id == draft.scene_id
                && t.record.id == change.after.record.id
                && t.record.asset_path == change.after.record.asset_path
                && t.record
                    .terrain_patch
                    .as_ref()
                    .is_some_and(|p| p.streamed_volume)
        });
        let Some(root) = streamed.map(|t| t.entity).or_else(|| {
            state
                .root
                .filter(|root| candidates.contains(root))
                .or_else(|| candidates.first().copied())
        }) else {
            editor.status = "等待三维地形正式根Ready后撤销创建…".into();
            return;
        };
        let render_record = streamed
            .map(|t| t.record.clone())
            .unwrap_or_else(|| change.after.record.clone());
        let hole = render_record
            .terrain_cutout(scene_bounds(&session).unwrap())
            .expect("已验证历史洞口")
            .unwrap();
        state.adoption = Some(Adoption {
            root,
            revision: session.revision,
            index: change.index,
            next: None,
            bytes: None,
            previous: request.target.clone(),
            history: Some((undoing, change.clone())),
            started: time.elapsed_secs_f64(),
            expected: Some(change.after.record.clone()),
            spatial: None,
        });
        request.target = Some(TerrainVolumePreviewTarget {
            scene_id: draft.scene_id.clone(),
            entity: root,
            record: render_record,
            world_cutout: hole,
            removal: true,
        });
    }
    editor.undo_requested = false;
    editor.redo_requested = false;
    editor.status = "正在后台恢复三维源，并等待地表与碰撞共同采用…".into();
}

#[cfg(test)]
#[path = "density_refresh_tests.rs"]
mod density_refresh_tests;
