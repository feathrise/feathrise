//! 所选对象完整载入后才交换作者槽；历史与异步结果始终带稳定对象身份。
use super::super::{EditorAuthoringSession, EditorCommand};
use super::{TerrainDocumentIdentity, TerrainDocumentTask, resources::Pending};
use crate::{resources::SceneEditorState, ui::terrain_objects::TerrainObjectsEditorState};
use bevy::{
    prelude::*,
    tasks::{IoTaskPool, futures::check_ready},
};
use feathrise_scene_content::{SceneTerrainObject, TerrainId};
use std::sync::Arc;

pub(crate) fn scope_history(session: &mut EditorAuthoringSession) {
    let Some(draft) = &mut session.draft else {
        return;
    };
    let id = draft
        .terrain_documents
        .active
        .as_ref()
        .map(|active| active.definition.id.clone());
    // 原主对象的持久洞穴历史继续由原后台门处理；新画布命令使用显式包装，不改变线性顺序。
    let start = draft
        .terrain_documents
        .scoped_through
        .min(session.undo.len());
    let end = (start + 64).min(session.undo.len());
    for command in &mut session.undo[start..end] {
        if matches!(command, EditorCommand::TerrainCanvas { .. } | EditorCommand::RootWater(_)) { continue; }
        if id.is_none() && (draft.manifest.terrains.is_some() || draft.manifest.terrain.is_empty())
            && (command.is_water_canvas() || matches!(command, EditorCommand::VegetationAuthoringChanged(_) | EditorCommand::ModelVegetationChanged { .. })) {
            if matches!(command, EditorCommand::VegetationAuthoringChanged(_) | EditorCommand::ModelVegetationChanged { .. }) {
                super::root_vegetation::mark_registration(draft);
            }
            draft.terrain_documents.active_dirty = true; // 根水参数/结构历史在停放后仍有独立JSON保存责任。
            let old = std::mem::replace(command, EditorCommand::TerrainChanged(Vec::new()));
            *command = EditorCommand::RootWater(Box::new(old));
        } else if let Some(id) = &id && command.is_terrain_canvas() {
            draft.terrain_documents.active_dirty = true;
            let old = std::mem::replace(command, EditorCommand::TerrainChanged(Vec::new()));
            *command = EditorCommand::TerrainCanvas {
                id: id.clone(),
                command: Box::new(old),
            };
        }
    }
    draft.terrain_documents.scoped_through = end;
}
pub(crate) fn request_focus(
    task: &mut TerrainDocumentTask,
    selection: &mut TerrainObjectsEditorState,
    draft: &super::super::EditorSceneDraft,
    id: &TerrainId,
) -> bool {
    let Some(index) = draft
        .manifest
        .terrains
        .as_ref()
        .and_then(|objects| objects.iter().position(|object| &object.id == id))
    else {
        return false;
    };
    task.requested = Some(id.clone());
    selection.selected = Some(id.clone());
    selection.selected_index = Some(index);
    true
}
fn identity(
    draft: &super::super::EditorSceneDraft,
    object: Arc<SceneTerrainObject>,
) -> TerrainDocumentIdentity {
    let folder = if object.id == TerrainId::legacy() {
        draft.folder.clone()
    } else {
        object.private_folder(&draft.scene_id)
    };
    TerrainDocumentIdentity {
        manifest: object.source_manifest(&draft.manifest),
        definition: object,
        folder,
    }
}
pub(super) fn focus(
    mut editor: ResMut<SceneEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut selection: ResMut<TerrainObjectsEditorState>,
    mut candidates: Option<ResMut<crate::viewport::LayerCandidateState>>,
    mut task: ResMut<TerrainDocumentTask>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut author_target: ResMut<feathrise_scene::TerrainObjectAuthoringTarget>,
    server: Option<Res<AssetServer>>,
    idle: super::resources::CanvasIdle,
    mut volume: Option<ResMut<super::super::volume_sculpt::VolumeAuthoringState>>,
    mut pending_water: Option<ResMut<crate::ui::water_layers::PendingClipmapWaterLayerCreate>>,
) {
    if session.water_preset_delete_pending { return; } // 保持引用检查过的当前/停放槽稳定，后台载入句柄仍由原槽持有。
    editor.terrain_canvas_target = selection.selected.clone();
    let water_revision = session.water_revision;
    if let Some(draft) = session.draft.as_mut() {
        super::root_water::remember_revision(draft, water_revision);
    }
    if task.retirement.as_mut().and_then(check_ready).is_some() {
        task.retirement = None;
    }
    if let Some(server) = server {
        for _ in 0..16 {
            let Some(path) = task.reloads.pop_front() else {
                break;
            };
            server.reload(path);
        }
    }
    scope_history(&mut session);
    if let Some(result) = task
        .pending
        .as_mut()
        .and_then(|pending| check_ready(&mut pending.task))
    {
        let pending = task.pending.take().unwrap();
        selection.documents_loading = false;
        if let Some(draft) = session
            .draft
            .as_mut()
            .filter(|draft| draft.scene_id == pending.scene_id)
        {
            draft.terrain_documents.loading = false;
        }
        if editor.open
            && selection.selected.as_ref() == Some(&pending.object.id)
            && session.revision == pending.revision
            && let Some(draft) = &mut session.draft
            && draft.scene_id == pending.scene_id
            && draft
                .manifest
                .terrains
                .as_ref()
                .and_then(|objects| objects.get(pending.index))
                == Some(pending.object.as_ref())
        {
            match result {
                Ok(mut canvas) => {
                    let same = draft
                        .terrain_documents
                        .active
                        .as_ref()
                        .is_some_and(|active| active.definition.id == pending.object.id);
                    if same {
                        draft.terrain_mesh = canvas.terrain_mesh;
                        draft.terrain_editing_index = canvas.terrain_editing_index;
                        draft.heights = canvas.heights;
                        draft.terrain_source_sha256 = canvas.terrain_source_sha256;
                        let puddles = draft.terrain_manifest().puddles.clone();
                        draft.terrain_documents.active = canvas.identity;
                        draft.terrain_manifest_mut().puddles = puddles;
                    } else {
                        if let Some(volume) = volume.as_deref_mut() {
                            volume.retarget_terrain_canvas();
                        }
                        super::transitions::park(draft);
                        draft.terrain_documents.values.remove(&pending.object.id);
                        draft
                            .terrain_documents
                            .dirty_stored
                            .remove(&pending.object.id);
                        draft.terrain_documents.saved.remove(&pending.object.id);
                        canvas.swap(draft);
                    }
                    author_target.scene_id = Some(draft.scene_id.clone());
                    author_target.terrain = Some(pending.object.id.clone());
                    session.revision = session.revision.wrapping_add(1);
                    // 完整画布切换统一使三个作者域的预览身份失效。
                    session.surface_revision = session.surface_revision.wrapping_add(1);
                    session.vegetation_revision = session.vegetation_revision.wrapping_add(1);
                    session.vegetation_structure_revision = session.vegetation_structure_revision.wrapping_add(1);
                    session.water_revision = session.water_revision.wrapping_add(1);
                    if let Some(candidates) = candidates.as_deref_mut() {
                        // 载入独立画布会推进作者修订，但已确认网格仍有效；同帧撤销旧点击资格并保留属性目标。
                        candidates.complete_terrain_canvas(&pending.object.id, pending.index, pending.revision, session.revision);
                    }
                    editor.cursor_world = None;
                    editor.brush_cursor_world = None;
                    if let Some(water) = pending_water.as_deref_mut() {
                        water.terrain_canvas_adopted(&pending.scene_id, &pending.object, pending.index, pending.revision, session.revision);
                    } // 仅旧nonEmpty兼容图等待原primary文件；独立根水从不经过此分支。
                    editor.status =
                        format!("已载入地形 {} 的独立作者画布", pending.object.display_name);
                }
                Err(error) => {
                    if let Some(water) = pending_water.as_deref_mut() {
                        water.terrain_canvas_failed(&pending.scene_id, &pending.object, pending.index, pending.revision);
                    }
                    editor.status = format!("地形画布未切换：{error}");
                }
            }
        } else {
            if let Some(water) = pending_water.as_deref_mut() {
                water.terrain_canvas_failed(&pending.scene_id, &pending.object, pending.index, pending.revision);
            }
            task.retirement = Some(bevy::tasks::AsyncComputeTaskPool::get().spawn(async move {
                drop(result);
            }));
        }
    }
    if task.pending.is_some()
        || task.retirement.is_some()
        || !editor.open
        || session.saving
        || selection.preparing()
        || selection.transforming
        || selection.brushing
        || mouse.pressed(MouseButton::Left)
        || idle.busy(&session)
        || volume.as_ref().is_some_and(|volume| volume.busy())
    {
        return;
    }
    if session.sparse_terrain_dirty {
        editor.status = "当前高度作者源尚未保存；保存完成后继续切换地形画布".into();
        return;
    }
    let history_len = session.undo.len();
    let revision = session.revision;
    let Some(draft) = &mut session.draft else {
        author_target.terrain = None;
        return;
    };
    if draft.manifest.terrains.is_none() {
        if let Err(error) = super::transitions::restore_legacy(draft) {
            editor.status = error;
            return;
        }
        author_target.scene_id = Some(draft.scene_id.clone());
        author_target.terrain = None;
        return;
    }
    let objects = draft.manifest.terrains.as_ref().unwrap();
    let target = task
        .requested
        .clone()
        .or_else(|| selection.selected.clone());
    let Some(id) = target else {
        author_target.scene_id = Some(draft.scene_id.clone());
        author_target.terrain = None;
        if draft.terrain_documents.active.is_some() {
            super::transitions::park(draft);
            super::root_water::restore(draft);
            super::root_vegetation::restore(draft);
            session.revision = session.revision.wrapping_add(1);
            session.surface_revision = session.surface_revision.wrapping_add(1);
            session.vegetation_revision = session.vegetation_revision.wrapping_add(1);
            session.vegetation_structure_revision = session.vegetation_structure_revision.wrapping_add(1);
            session.water_revision = session.water_revision.wrapping_add(1);
        }
        return;
    };
    let Some(index) = selection.selected_index else {
        return;
    };
    let Some(object) = objects.get(index).filter(|object| object.id == id) else {
        task.requested = None;
        selection.documents_loading = false;
        return;
    };
    // 稳定选择只核对固定槽与当前定义，不逐帧扫描完整对象集合或重建源清单。
    if draft
        .terrain_documents
        .active
        .as_ref()
        .is_some_and(|active| active.definition.as_ref() == object)
    {
        if author_target.scene_id.as_ref() != Some(&draft.scene_id) {
            author_target.scene_id = Some(draft.scene_id.clone());
        }
        if author_target.terrain.as_ref() != Some(&object.id) {
            author_target.terrain = Some(object.id.clone());
        }
        task.requested = None;
        selection.documents_loading = false;
        return;
    }
    let object = object.clone();
    // 显式容器的primary也必须完整载入；只有ensure_primary_canvas已认领的旧清单迁移能复用原作者槽。
    // 仅凭保留ID认领空槽会跳过地表、植被与水域文件，并把尚未加载的画布误报为就绪。
    if let Some(active) = &draft.terrain_documents.active
        && active.definition.id == object.id
        && active.definition.terrain == object.terrain
        && active.definition.voxel == object.voxel
    {
        let mut updated = identity(draft, Arc::new(object));
        updated.manifest.puddles = active.manifest.puddles.clone();
        author_target.scene_id = Some(draft.scene_id.clone());
        author_target.terrain = Some(updated.definition.id.clone());
        draft.terrain_documents.active = Some(updated);
        task.requested = None;
        selection.documents_loading = false;
        return;
    }
    if draft.terrain_documents.scoped_through < history_len {
        return;
    }
    let same = draft
        .terrain_documents
        .active
        .as_ref()
        .is_some_and(|active| active.definition.id == id);
    let cached = draft.terrain_documents.values.get(&id).cloned();
    let saved = draft.terrain_documents.saved.contains(&id);
    let source = identity(draft, Arc::new(object));
    let map = draft.scene_id.clone();
    let object = source.definition.clone();
    task.requested = None;
    selection.documents_loading = true;
    draft.terrain_documents.loading = true;
    task.pending = Some(Pending {
        scene_id: map,
        revision,
        index,
        object,
        task: IoTaskPool::get().spawn(super::load::load(source, cached, same, saved)),
    });
}
