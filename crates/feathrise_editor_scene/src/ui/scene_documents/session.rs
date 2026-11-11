//! 场景作者所有权的暂停与恢复；不深复制高度、像素、外部正文或撤销版本。

use super::history::{SceneHistoryOrder, merge_history, split_history};
use super::resources::{SceneDocumentPreview, SceneDocumentSnapshot, SceneDocuments, SceneHistoryTransfer};
use super::types::SceneDocumentUi;
use crate::authoring::AuthoringRetirement;
use crate::authoring::{
    BoundaryEditState, EditorAuthoringSession, SparseTerrainAuthoringState, SurfaceBrushStroke,
    SurfaceLayerEditTask, TerrainBrushStroke, VegetationBrushStroke, VegetationCompileState,
    WaterFileHandoff, WaterMaskMaterializeTask, WaterNetworkEditState,
};
use crate::resources::{EditorViewportSelectionState, SceneEditorState};
use crate::ui::gltf_browser::EditorGltfBrowserState;
use bevy::{ecs::system::SystemParam, prelude::*, tasks::AsyncComputeTaskPool};
use feathrise_scene_content::{SceneAuthorPreview, SparseHeightLiveOverride};

/// 独立旧入口未装配文档功能时保持兼容；正式F12由Plugin初始化全部资源。
#[derive(SystemParam)]
pub(crate) struct SceneDocumentTransition<'w> {
    pub(crate) residency: Option<Res<'w, feathrise_scene::ResidentScenes>>,
    pub(crate) activity: Option<Res<'w, feathrise_scene_core::residency::SceneResidencyActivity>>,
    pub(crate) documents: Option<ResMut<'w, SceneDocuments>>,
    pub(super) sparse: Option<ResMut<'w, SparseTerrainAuthoringState>>,
    pub(super) heights: Option<ResMut<'w, SparseHeightLiveOverride>>,
    pub(super) domain: Option<Res<'w, crate::game_authoring::scene_interaction::SceneInteractionState>>,
    pub(super) selection: Option<ResMut<'w, EditorViewportSelectionState>>,
    browser: Option<Res<'w, EditorGltfBrowserState>>,
    compile: Option<Res<'w, VegetationCompileState>>,
    pub(super) retire: Option<ResMut<'w, AuthoringRetirement>>,
    pub(super) preview: Option<ResMut<'w, SceneAuthorPreview>>,
    terrain_stroke: Option<Res<'w, TerrainBrushStroke>>,
    surface_stroke: Option<Res<'w, SurfaceBrushStroke>>,
    vegetation_stroke: Option<Res<'w, VegetationBrushStroke>>,
    vegetation_cross: Option<Res<'w,crate::authoring::vegetation_paint_scope::VegetationPaintState>>,
    surface_edit: Option<Res<'w, SurfaceLayerEditTask>>,
    materialize: Option<Res<'w, WaterMaskMaterializeTask>>,
    water_edit: Option<Res<'w, WaterNetworkEditState>>,
    boundary_edit: Option<Res<'w, BoundaryEditState>>,
    water_handoff: Option<Res<'w, WaterFileHandoff>>,
    courtyard: Option<Res<'w, crate::authoring::courtyards::CourtyardAuthoringState>>,
    buildings: crate::authoring::buildings::BuildingDocumentGate<'w>,
    vegetation_allocation:
        Option<Res<'w, crate::ui::vegetation_layers::VegetationBlankLayerAllocation>>,
    water_stroke: Option<Res<'w, crate::authoring::WaterBrushStroke>>,
    puddles: Option<Res<'w, crate::authoring::PuddleAuthoringState>>,
    numeric: Option<Res<'w, feathrise_editor_ui::ActiveNumericInput>>,
    subscenes: Option<Res<'w, crate::ui::subscenes::SubsceneEditorState>>,
    terrain_objects: Option<Res<'w, crate::ui::terrain_objects::TerrainObjectsEditorState>>,
    subscene_transform: Option<Res<'w, crate::viewport::subscenes::SubsceneTransformState>>,
}

impl SceneDocumentTransition<'_> {
    pub(crate) fn busy(&self) -> bool {
        if self.subscenes.as_ref().is_some_and(|state| state.busy())
            || self.vegetation_cross.as_ref().is_some_and(|state|state.busy())
            || self.terrain_objects.as_ref().is_some_and(|state| state.busy())
            || self.subscene_transform.as_ref().is_some_and(|state| state.owns_mouse()) { return true; }
        self.documents
            .as_ref()
            .is_some_and(|docs| docs.history_task.is_some())
            || self
                .sparse
                .as_ref()
                .is_some_and(|state| !state.document_can_park())
            || self
                .compile
                .as_ref()
                .is_some_and(|state| state.task.is_some())
            || self
                .terrain_stroke
                .as_ref()
                .is_some_and(|stroke| stroke.active)
            || self
                .surface_stroke
                .as_ref()
                .is_some_and(|stroke| stroke.active || stroke.pending_sample.is_some())
            || self
                .vegetation_stroke
                .as_ref()
                .is_some_and(|stroke| stroke.active)
            || self
                .surface_edit
                .as_ref()
                .is_some_and(|state| state.is_pending())
            || self
                .materialize
                .as_ref()
                .is_some_and(|state| state.task.is_some() || state.terrain_task.is_some())
            || self
                .water_edit
                .as_ref()
                .is_some_and(|state| state.dragging())
            || self
                .boundary_edit
                .as_ref()
                .is_some_and(|state| state.dragging())
            || self
                .water_handoff
                .as_ref()
                .is_some_and(|state| state.blocks_new_save_for(None))
            || self
                .courtyard
                .as_ref()
                .is_some_and(|state| state.operation_pending() || state.visual_sync_pending())
            || self.buildings.busy()
            || self
                .vegetation_allocation
                .as_ref()
                .is_some_and(|state| state.busy())
            || self
                .water_stroke
                .as_ref()
                .is_some_and(|stroke| stroke.active)
            || self.puddles.as_ref().is_some_and(|state| state.busy())
            || self
                .numeric
                .as_ref()
                .is_some_and(|state| state.entity.is_some())
            || self.domain.as_ref().is_some_and(|state| state.busy())

    }
}

/// 在正式切换场景采用点暂停源文档；目标清单坏、保存或整笔未完成时不能调用。
pub(crate) fn park_document(
    state: &mut SceneDocumentTransition,
    editor: &mut SceneEditorState,
    session: &mut EditorAuthoringSession,
    target: &feathrise_scene_content::SceneId,
    commands: &mut Commands,
) {
    let Some(documents) = state.documents.as_mut() else {
        return;
    };
    let source = session.draft.as_ref().map(|draft| draft.scene_id.clone());
    let dirty_before = editor.dirty;
    let data_dirty = session.has_scene_data_changes();
    let undo = std::mem::take(&mut session.undo);
    let redo = std::mem::take(&mut session.redo);
    let resident = state.residency.is_some()
        && state
            .activity
            .as_ref()
            .is_some_and(|activity| activity.enabled);
    if let Some(scene_id) = source.as_ref() {
        // Entity身份只能属于当前世界；后台析构索引，回来时由既有预览系统重新生成。
        if !resident && let Some(retire) = state.retire.as_mut() {
            retire.push(std::mem::take(&mut session.placement_previews));
            retire.push(std::mem::take(&mut session.portal_previews));
        }
        let mut ui =
            SceneDocumentUi::capture(editor, state.browser.as_ref().expect("场景树已初始化"));
        if let Some(subscenes) = state.subscenes.as_ref() {
            ui.subscene = subscenes.selected.clone();
        }
        if let Some((map, pending)) = documents.pending_selection.take()
            && &map == scene_id
        {
            ui.placement = pending.placement;
            ui.mesh = pending.mesh; // 场景树仍在异步准备时，连续切标签也不能丢失原稳定选择。
            ui.subscene = pending.subscene;
        }
        let draft = session.draft.as_ref().unwrap();
        documents.upsert(
            scene_id.clone(),
            draft.manifest.display_name.clone(),
            editor.dirty && !session.is_unedited_load_migration_for(scene_id),
        );
        let mut selection =
            std::mem::take(&mut **state.selection.as_mut().expect("选择状态已初始化"));
        let stable_selection = if resident {
            std::mem::take(&mut selection)
        } else {
            EditorViewportSelectionState {
                selected_vegetation_scene_id: selection.selected_vegetation_scene_id.take(),
                selected_vegetation_output_id: selection.selected_vegetation_output_id.take(),
                ..default()
            }
        };
        if let Some(retire) = state.retire.as_mut() {
            retire.push(selection);
        }
        let snapshot = SceneDocumentSnapshot {
            session: std::mem::take(session),
            sparse: std::mem::take(&mut **state.sparse.as_mut().expect("地形编辑数据已初始化")),
            heights: std::mem::take(&mut **state.heights.as_mut().expect("高度覆盖已初始化")),
            selection: stable_selection,
            ui,
            dirty: editor.dirty,
            history_order: SceneHistoryOrder::default(),
        };
        if let Some(previous) = documents.parked.insert(scene_id.clone(), snapshot) {
            if let Some(retire) = state.retire.as_mut() {
                retire.push(previous);
            }
        }
    }
    if let Some(map) = source.as_ref() {
        let map = map.clone();
        commands.queue(move |world: &mut World| {
            if let Some(adapter) = world.get_resource::<crate::authoring::domain_documents::SceneDomainAdapter>().copied() { (adapter.park)(world, map); }
        }); // 同一系统的延迟采用边界先暂停具体领域，再允许目标文档推进。
    }
    let mut target_document = documents.parked.remove(target);
    if resident && let Some(handles) = documents.preview_storage.take() {
        documents
            .parked_preview_storage
            .insert(handles.scene_id.clone(), handles);
    }
    let warm = state
        .residency
        .as_ref()
        .is_some_and(|scenes| scenes.sleeping(target) || scenes.restored(target));
    let target = target.clone();
    // 【功能设计】全局天体命令只有一份，随活动工作区交接；场景命令留在各自文档。
    // 分离、合并及Vec分配随历史规模增长，全部放入唯一后台任务，主线程只移动所有权。
    documents.history_task = Some(AsyncComputeTaskPool::get().spawn(async move {
        let (mut target_undo, mut target_redo) = target_document
            .as_mut()
            .map(|cached| {
                (
                    std::mem::take(&mut cached.session.undo),
                    std::mem::take(&mut cached.session.redo),
                )
            })
            .unwrap_or_default();
        let preview = target_document.as_ref().filter(|_| !warm).map(|cached| {
            let draft = cached.session.draft.as_ref().expect("后台标签持有完整草稿");
            SceneDocumentPreview {
                placements: draft.placements.clone(),
                portals: draft.portals.clone(),
                boundaries: draft.boundaries.clone(),
            }
        }); // 整图预览快照只在唯一后台任务构建，主线程不复制全部placement或边界。
        if source.as_ref() == Some(&target) {
            // 取消后冷重开同图时，源快照就是目标文档。整栈原样交还，不能把局部历史拆走后
            // 又用只含全局命令的目标栈覆盖它；原局部/全局顺序和redo全部保持。
            return SceneHistoryTransfer {
                source: None, target, target_undo: undo, target_redo: redo, target_document, preview,
                source_undo: Vec::new(), source_redo: Vec::new(),
                source_order: SceneHistoryOrder::default(), source_dirty: false,
            };
        }
        let (mut source_undo, global_undo, undo_order) = split_history(undo);
        let (mut source_redo, global_redo, redo_order) = split_history(redo);
        let source_dirty =
            dirty_before && (data_dirty || !source_undo.is_empty() || !source_redo.is_empty());
        if source.is_none() {
            source_undo.clear();
            source_redo.clear();
        } // 关闭文档的场景历史在后台释放。
        let order = target_document
            .as_mut()
            .map(|cached| std::mem::take(&mut cached.history_order))
            .unwrap_or_default();
        target_undo = merge_history(target_undo, global_undo, order.undo);
        target_redo = merge_history(target_redo, global_redo, order.redo);
        SceneHistoryTransfer {
            source,
            target,
            source_undo,
            source_redo,
            target_undo,
            target_redo,
            target_document,
            preview,
            source_order: SceneHistoryOrder {
                undo: undo_order,
                redo: redo_order,
            },
            source_dirty,
        }
    }));
    editor.dirty = false;
    editor.undo_requested = false;
    editor.redo_requested = false;
    editor.cursor_world = None;
    editor.brush_cursor_world = None;
}

#[derive(SystemParam)]
pub(crate) struct SceneDocumentLoad<'w> {
    pub(crate) residency: Option<Res<'w, feathrise_scene::ResidentScenes>>,
    pub(crate) documents: Option<ResMut<'w, SceneDocuments>>,
    sparse: Option<ResMut<'w, SparseTerrainAuthoringState>>,
    heights: Option<ResMut<'w, SparseHeightLiveOverride>>,
    selection: Option<ResMut<'w, EditorViewportSelectionState>>,
}

/// 草稿经过原采用/预览回收路径后，原子恢复缓存作者状态；不复活任何旧相机或世界实体。
pub(crate) fn restore_document(
    state: &mut SceneDocumentLoad,
    cached: Option<SceneDocumentSnapshot>,
    session: &mut EditorAuthoringSession,
    editor: &mut SceneEditorState,
    commands: &mut Commands,
    mut retire: Option<&mut AuthoringRetirement>,
    global_dirty: bool,
) {
    let Some(documents) = state.documents.as_mut() else {
        return;
    };
    let scene_id = session.draft.as_ref().unwrap().scene_id.clone();
    let warm = state
        .residency
        .as_ref()
        .is_some_and(|scenes| scenes.restored(&scene_id));
    if let Some(mut cached) = cached {
        let draft = session.draft.take();
        let only_loaded_migration =
            cached.session.loaded_migration_revision == Some(cached.session.revision);
        let epoch = cached
            .session
            .revision
            .max(session.revision)
            .wrapping_add(1);
        cached.session.draft = draft;
        if !warm {
            cached.session.placement_previews.clear();
            cached.session.portal_previews.clear(); // 冷载不能复用上一运行时世代的Entity索引。
            cached.session.revision = epoch;
            // 所有派生域同步换代，旧图的迟到CPU/GPU结果不能在返回同SceneId后重新获得资格。
            for revision in [
                &mut cached.session.placement_structure_revision,
                &mut cached.session.portal_revision,
                &mut cached.session.terrain_revision,
                &mut cached.session.surface_revision,
                &mut cached.session.vegetation_revision,
                &mut cached.session.water_revision,
                &mut cached.session.environment_revision,
                &mut cached.session.boundary_revision,
            ] {
                *revision = (*revision).max(epoch).wrapping_add(1);
            }
            cached.session.vegetation_compiled_revision =
                cached.session.vegetation_revision.wrapping_sub(1);
            // 无编辑的兼容补齐随作者会话换代延续，关闭标签沿用原有免提示规则。
            cached.session.loaded_migration_revision = only_loaded_migration.then_some(epoch);
        } // 完整场景恢复保留原作者代次，原Entity索引与在途派生任务仍属于该场景。
        *session = cached.session;
        **state.sparse.as_mut().expect("地形编辑数据已初始化") = cached.sparse;
        if !warm {
            cached
                .heights
                .retarget(Some(scene_id.clone()), session.terrain_revision);
        }
        **state.heights.as_mut().expect("高度覆盖已初始化") = cached.heights;
        **state.selection.as_mut().expect("选择状态已初始化") = cached.selection;
        let restore_scene = scene_id.clone();
        commands.queue(move |world: &mut World| {
            if let Some(adapter) = world.get_resource::<crate::authoring::domain_documents::SceneDomainAdapter>().copied() { (adapter.restore)(world, restore_scene, warm); }
        }); // 游戏自己的停放状态与完整草稿在同一采用边界恢复。
        cached.ui.restore(editor);
        documents.pending_selection = Some((scene_id.clone(), cached.ui));
        editor.dirty = cached.dirty || global_dirty;
        editor.status = format!("已恢复场景{}的草稿、撤销记录与工具选择", scene_id);
    }
    if let Some(history) = documents.pending_history.take() {
        debug_assert_eq!(history.target, scene_id);
        session.undo = history.target_undo;
        session.redo = history.target_redo;
        if let Some(retire) = retire.as_mut() {
            retire.push((
                history.source_undo,
                history.source_redo,
                history.source_order,
            ));
        }
    }
}
