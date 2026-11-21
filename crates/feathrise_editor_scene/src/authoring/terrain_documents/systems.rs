//! 所选对象完整载入后才交换作者槽；历史与异步结果始终带稳定对象身份。
use super::super::{EditorAuthoringSession, EditorCommand};
use super::{TerrainDocumentIdentity, TerrainDocumentTask, resources::Pending};
use crate::{resources::SceneEditorState, ui::terrain_objects::TerrainObjectsEditorState};
use bevy::{
    prelude::*,
    tasks::{IoTaskPool, futures::check_ready},
};
use feathrise_scene_content::{SceneTerrainObject, TerrainId};
use std::sync::{Arc, Weak};

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
    task.vegetation = None;
    task.vegetation_ready = None;
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
    catalog: crate::ui::scene_layers::VegetationCatalogAssets,
    idle: super::resources::CanvasIdle,
    mut volume: Option<ResMut<super::super::volume_sculpt::VolumeAuthoringState>>,
    mut pending_water: Option<ResMut<crate::ui::water_layers::PendingClipmapWaterLayerCreate>>,
    mut tree_menu: Option<ResMut<crate::ui::scene_context_menu::SceneContextMenuState>>,
    vegetation_events: Option<Res<Messages<AssetEvent<feathrise_scene_content::SceneVegetationAsset>>>>,
    mut vegetation_cursor: Local<bevy::ecs::message::MessageCursor<AssetEvent<feathrise_scene_content::SceneVegetationAsset>>>,
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
    if let Some(server) = catalog.server.as_deref() {
        for _ in 0..16 {
            let Some(path) = task.reloads.pop_front() else {
                break;
            };
            server.reload(path);
        }
    }
    scope_history(&mut session);
    let requested_asset = task.vegetation.as_ref().and_then(|intent| intent.source()).map(|(_, asset)| asset);
    let source_changed = vegetation_events.as_ref().is_some_and(|events| {
        let changed = vegetation_cursor.read(events).any(|event| match event {
            AssetEvent::Modified { id } | AssetEvent::Removed { id } => Some(*id) == requested_asset,
            _ => false,
        });
        vegetation_cursor.clear(events);
        changed
    });
    if task.vegetation.as_ref().is_some_and(|intent| source_changed
        || intent.selection_ticket != candidates.as_ref().map(|state| state.selection_ticket())
        || !intent.target.current(&editor, &session)
        || session.draft.as_ref().and_then(|draft| catalog.row_definition(draft, &intent.target.key)).is_none()
        || editor.new_scene_dialog_open || editor.save_requested)
    {
        task.vegetation = None;
    }
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
        let layer_request = pending.vegetation && task.vegetation.as_ref().is_some_and(|intent| {
            intent.source().is_some_and(|(index, _)| index == pending.index)
                && intent.target.scene_id == pending.scene_id && intent.target.revision == pending.revision
        });
        let mut vegetation = if layer_request { task.vegetation.take() } else { None };
        let may_adopt = if pending.vegetation {
            layer_request && !idle.busy(&session) && !session.sparse_terrain_dirty
                && !selection.transforming && !selection.brushing
                && volume.as_ref().is_none_or(|state| !state.busy())
        } else {
            selection.selected.as_ref() == Some(&pending.object.id)
        };
        if editor.open
            && may_adopt
            && session.revision == pending.revision
            && let Some(draft) = &mut session.draft
            && draft.scene_id == pending.scene_id
            && draft
                .manifest
                .terrains
                .as_ref()
                .and_then(|objects| objects.get(pending.index))
                == Some(pending.object.as_ref())
            // 水域保存失败可恢复停放槽的脏责任而不推进作者修订；不能用在途旧副本覆盖它。
            && (!pending.vegetation || pending.parked.as_ref().map(Weak::as_ptr)
                == draft.terrain_documents.values.get(&pending.object.id).map(Arc::as_ptr))
        {
            match result {
                Ok(mut canvas) if vegetation.as_mut().is_none_or(|intent| intent.adopt(canvas.vegetation.as_ref())) => {
                    if vegetation.as_ref().is_some_and(|intent| intent.action == super::TreeVegetationAction::Visibility) {
                        // 眼睛需要真实作者密度给原临时预览，但不更换活动画布、选中对象或作者修订。
                        let retired = draft.terrain_documents.values.insert(pending.object.id.clone(), Arc::new(canvas));
                        if retired.is_some() {
                            task.retirement = Some(bevy::tasks::AsyncComputeTaskPool::get().spawn(async move { drop(retired); }));
                        }
                        task.vegetation_ready = vegetation;
                        return;
                    }
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
                        let retired = draft.terrain_documents.values.remove(&pending.object.id);
                        if pending.vegetation && retired.is_some() {
                            task.retirement = Some(bevy::tasks::AsyncComputeTaskPool::get().spawn(async move { drop(retired); }));
                        }
                        draft
                            .terrain_documents
                            .dirty_stored
                            .remove(&pending.object.id);
                        draft.terrain_documents.saved.remove(&pending.object.id);
                        canvas.swap(draft);
                    }
                    author_target.scene_id = Some(draft.scene_id.clone());
                    author_target.terrain = Some(pending.object.id.clone());
                    if vegetation.is_some() {
                        selection.scene_id = Some(draft.scene_id.clone());
                        selection.selected = Some(pending.object.id.clone());
                        selection.selected_index = Some(pending.index);
                        editor.terrain_canvas_target = Some(pending.object.id.clone());
                    }
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
                    if let Some(mut intent) = vegetation {
                        intent.target.revision = session.revision;
                        intent.selection_ticket = candidates.as_ref().map(|state| state.selection_ticket());
                        if let crate::ui::scene_context_menu::SceneObjectKey::Vegetation { terrain, id, .. } = &intent.target.key {
                            intent.target.hidden = editor.vegetation_visibility.hidden_in(&intent.target.scene_id, terrain.as_ref())
                                .is_some_and(|hidden| hidden.contains(id));
                        }
                        if intent.action.selects() && let Some(menu) = tree_menu.as_deref_mut() {
                            menu.rebind_loaded_vegetation(&intent.target, pending.revision);
                        }
                        task.vegetation_ready = Some(intent);
                    }
                    editor.status =
                        format!("已载入地形 {} 的独立作者画布", pending.object.display_name);
                }
                Ok(canvas) => {
                    editor.status = "植被图层已变化，请从场景树重新选择".into();
                    task.retirement = Some(bevy::tasks::AsyncComputeTaskPool::get().spawn(async move { drop(canvas); }));
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
    let layer_index = task.vegetation.as_ref().and_then(|intent| intent.source()).map(|(index, _)| index);
    let target = if let Some(index) = layer_index {
        objects.get(index).map(|object| object.id.clone())
    } else { task
        .requested
        .clone()
        .or_else(|| selection.selected.clone()) };
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
    let Some(index) = layer_index.or(selection.selected_index) else {
        return;
    };
    let Some(object) = objects.get(index).filter(|object| object.id == id) else {
        task.requested = None;
        selection.documents_loading = false;
        return;
    };
    // 稳定选择只核对固定槽与当前定义，不逐帧扫描完整对象集合或重建源清单。
    if layer_index.is_none() && draft
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
    if layer_index.is_none()
        && let Some(active) = &draft.terrain_documents.active
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
    let saved = draft.terrain_documents.saved.contains(&id) && !draft.terrain_documents.dirty_stored.contains(&id);
    let parked = draft.terrain_documents.values.get(&id);
    let mut cached = parked;
    if layer_index.is_some() && cached.is_some_and(|canvas| canvas.identity.as_ref().is_none_or(|old|
        old.definition.vegetation_path != object.vegetation_path)) {
        if draft.terrain_documents.dirty_stored.contains(&id) || !saved && cached.is_some_and(|canvas| canvas.dirty()) {
            task.vegetation = None;
            editor.status = "该地形的植被来源已更换，请先处理尚未保存的旧画布".into();
            return;
        }
        cached = None; // 已替换源路径，只能从当前权威文件恢复，不能复用旧槽的同名图层。
    }
    let source = identity(draft, Arc::new(object));
    let map = draft.scene_id.clone();
    let object = source.definition.clone();
    task.requested = None;
    // 冷图层的浏览命令仍能被显式换选撤销，作者输入由完整画布的加载门保持关闭。
    selection.documents_loading = layer_index.is_none();
    draft.terrain_documents.loading = true;
    task.pending = Some(Pending {
        scene_id: map,
        revision,
        index,
        object,
        vegetation: layer_index.is_some(),
        parked: layer_index.and(parked).map(Arc::downgrade),
        task: IoTaskPool::get().spawn(super::load::load(source, cached.cloned(), same, saved)),
    });
}
