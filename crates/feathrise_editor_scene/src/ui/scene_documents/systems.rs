//! 标签动作、完整历史交接和显式关闭；所有保存沿用原事务及成功回执。

use feathrise_editor_ui::EditorButtonClick;
use super::{resources::SceneDocuments, session::SceneDocumentTransition, types::SceneDocumentButton};
use crate::authoring::{EditorAuthoringSession, GlobalEnvironmentAuthoringState};
use crate::resources::{SceneBrowserDragState, SceneDeletionTask, SceneEditorState};
use crate::types::{SceneSwitchConfirmation, SceneSwitchStage};
use crate::ui::vegetation_preset_editor::{PresetWorkspaceTab, VegetationPresetEditorState};
use bevy::{
    ecs::system::SystemParam,
    prelude::*,
    tasks::{AsyncComputeTaskPool, futures::check_ready},
};
use feathrise_scene_content::{
    SceneAuthorPreview, SceneAuthorPreviewHandles, SceneBoundariesAsset, ScenePlacementsAsset,
    ScenePortalsAsset,
};

#[derive(SystemParam)]
pub(crate) struct SceneDocumentPreviewPublish<'w> {
    placements: ResMut<'w, Assets<ScenePlacementsAsset>>,
    portals: ResMut<'w, Assets<ScenePortalsAsset>>,
    boundaries: ResMut<'w, Assets<SceneBoundariesAsset>>,
    preview: ResMut<'w, SceneAuthorPreview>,
    retire: ResMut<'w, crate::authoring::AuthoringRetirement>,
}

impl SceneDocumentPreviewPublish<'_> {
    fn retire_storage(&mut self, documents: &mut SceneDocuments) {
        let Some(old) = documents.preview_storage.take() else {
            return;
        };
        self.retire_handles(old);
    }

    fn retire_handles(&mut self, old: SceneAuthorPreviewHandles) {
        if let Some(value) = self.placements.remove(old.placements.id()) {
            self.retire.push(value);
        }
        if let Some(value) = self.portals.remove(old.portals.id()) {
            self.retire.push(value);
        }
        if let Some(value) = old
            .boundaries
            .and_then(|handle| self.boundaries.remove(handle.id()))
        {
            self.retire.push(value);
        }
    }
}

pub(crate) fn poll_scene_document_history(
    mut documents: ResMut<SceneDocuments>,
    mut publish: SceneDocumentPreviewPublish,
    mut workspace: Option<ResMut<VegetationPresetEditorState>>,
    mut session: ResMut<EditorAuthoringSession>,
    scenes: Option<Res<feathrise_scene::ResidentScenes>>,
) {
    if session.water_preset_delete_pending || session.model_vegetation_busy { return; }
    if let Some(task) = documents.cleanup_history.as_mut() && let Some((mut undo, mut redo)) = check_ready(task) {
        documents.cleanup_history = None;
        undo.append(&mut session.undo); redo.append(&mut session.redo);
        session.undo = undo; session.redo = redo;
        if let Some(draft) = session.draft.as_mut() { draft.terrain_documents.scoped_through = 0; }
        session.saving = false;
    }
    if let Some((map, _)) = documents.retiring_preview_storage.first_key_value()
        && scenes.as_ref().is_none_or(|scenes| !scenes.contains(map))
    {
        let (_, handles) = documents.retiring_preview_storage.pop_first().unwrap();
        publish.retire_handles(handles);
    }
    if scenes.as_ref().is_some_and(|scenes| !scenes.source_ready()) {
        return;
    } // 源场景资源先移交，目标预览才允许进入活动槽。
    if documents.preview_storage.is_some() && publish.preview.active.is_none() {
        publish.retire_storage(&mut documents); // 保存/关闭已撤销临时输入，完整快照移到后台释放。
    }
    let Some(task) = documents.history_task.as_mut() else {
        return;
    };
    let Some(mut result) = check_ready(task) else {
        return;
    };
    documents.history_task = None;
    let warm = scenes
        .as_ref()
        .is_some_and(|scenes| scenes.sleeping(&result.target) || scenes.restoring(&result.target));
    if !warm {
        publish.preview.active = None;
        publish.retire_storage(&mut documents);
    } else {
        documents.preview_storage = documents.parked_preview_storage.remove(&result.target);
    }
    if let Some(preview) = result.preview.take() {
        let handles = SceneAuthorPreviewHandles {
            scene_id: result.target.clone(),
            placements: publish.placements.add(preview.placements),
            portals: publish.portals.add(preview.portals),
            boundaries: preview
                .boundaries
                .map(|value| publish.boundaries.add(value)),
        };
        publish.preview.active = Some(handles.clone());
        documents.preview_storage = Some(handles);
    }
    if let Some(target) = result.target_document.take() {
        documents.parked.insert(result.target.clone(), target);
    }
    if let Some(source) = result.source.as_ref()
        && let Some(cached) = documents.parked.get_mut(source)
    {
        cached.session.undo = std::mem::take(&mut result.source_undo);
        cached.session.redo = std::mem::take(&mut result.source_redo);
        cached.history_order = std::mem::take(&mut result.source_order);
        cached.dirty = result.source_dirty; // 共享全局脏位不冻结进场景；在别图保存全局后，返回干净场景仍保持干净。
        let name = cached
            .session
            .draft
            .as_ref()
            .unwrap()
            .manifest
            .display_name
            .clone();
        let tab_dirty =
            result.source_dirty && !cached.session.is_unedited_load_migration_for(source);
        if documents.upsert(source.clone(), name, tab_dirty)
            && let Some(workspace) = workspace.as_mut()
        {
            workspace.touch_ui();
        }
    }
    documents.pending_history = Some(result); // 目标完整草稿采用后才移动这两份栈，不提前发布半套状态。
}

pub(crate) fn handle_scene_document_buttons(
    buttons: Query<(&EditorButtonClick, &SceneDocumentButton), Changed<EditorButtonClick>>,
    mut documents: ResMut<SceneDocuments>,
    mut editor: ResMut<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    mut workspace: ResMut<VegetationPresetEditorState>,
    deletion: Res<SceneDeletionTask>,
    mut drag: ResMut<SceneBrowserDragState>,
    mut focus: Option<ResMut<bevy::input_focus::InputFocus>>,
    settings: Res<crate::resources::SceneEditorSettingsState>,
    numeric: Res<crate::resources::SceneEditorNumericScrubMouseGuard>,
) {
    if session.water_preset_delete_pending || session.model_vegetation_busy { return; } // 不在整笔后台采用前移动源文档。
    if !editor.open
        || editor.scene_transition_in_progress
        || editor.pending_scene_transition.is_some()
        || editor.scene_switch_confirmation.is_some()
        || editor.new_scene_dialog_open
        || editor.close_requested
        || deletion.busy()
        || session.saving
        || editor.save_requested
        || settings.open
        || numeric.blocked()
    {
        return;
    }
    for (interaction, button) in &buttons {
        if !interaction.is_clicked() {
            continue;
        }
        let (SceneDocumentButton::Activate(map) | SceneDocumentButton::Close(map)) = button;
        if !documents.contains(map) {
            continue;
        } // 整树重建前的旧按钮不能重新打开已关闭文档。
        drag.press = None;
        drag.blocked = true;
        editor.suppress_world_pick_this_frame = true;
        if let Some(focus) = focus.as_mut() {
            focus.clear();
        }
        if matches!(button, SceneDocumentButton::Close(_)) {
            documents.close_requested = Some(map.clone()); // 后台标签先采用自己的草稿，再用原保存链关闭。
        }
        if editor.selected_scene_id.as_ref() != Some(map) {
            editor.queue_scene_transition(map.clone(), "正在切换场景标签；原文档修改会保留");
        } else {
            workspace.active_tab = PresetWorkspaceTab::Scene;
            workspace.touch_ui();
        }
        return; // 一帧只消费一个标签动作。
    }
}

/// 元数据只有真实变化才重建标签；大场景作者源和历史不参与标签刷新。
pub(crate) fn sync_scene_documents(
    mut commands: Commands,
    mut documents: ResMut<SceneDocuments>,
    editor: Res<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    global: Res<GlobalEnvironmentAuthoringState>,
    mut workspace: ResMut<VegetationPresetEditorState>,
) {
    if !(editor.is_changed() || session.is_changed() || global.is_changed()) {
        return;
    }
    let Some(draft) = session.draft.as_ref() else {
        return;
    };
    if editor.selected_scene_id.as_ref() != Some(&draft.scene_id) {
        return;
    }
    let map = draft.scene_id.clone();
    commands.queue(move |world: &mut World| feathrise_scene::mark_resident_document_open(world, map)); // 重建运行时后元数据可不变，但新的场景仍属于这个已打开标签。
    // 【功能设计】标签星号表示作者修改；兼容加载补齐保留内部保存标记，不能直接映射为星号。
    let tab_dirty =
        global.dirty || (editor.dirty && !session.is_unedited_load_migration_for(&draft.scene_id));
    if documents
        .active_metadata
        .as_ref()
        .is_some_and(|(map, name, dirty)| {
            map == &draft.scene_id && name == &draft.manifest.display_name && *dirty == tab_dirty
        })
    {
        return;
    }
    if documents.upsert(
        draft.scene_id.clone(),
        draft.manifest.display_name.clone(),
        tab_dirty,
    ) {
        workspace.touch_ui();
    }
    documents.active_metadata = Some((
        draft.scene_id.clone(),
        draft.manifest.display_name.clone(),
        tab_dirty,
    ));
}

/// 玩法传送后再次打开F12也从统一事务交接旧作者文档，不能让加载直接替换上一张草稿。
pub(crate) fn preserve_document_on_external_scene_change(
    mut editor: ResMut<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    documents: Res<SceneDocuments>,
) {
    if !editor.open
        || editor.pending_scene_transition.is_some()
        || editor.scene_transition_in_progress
        || documents.history_task.is_some()
    {
        return;
    }
    if let (Some(target), Some(source)) = (editor.selected_scene_id.as_ref(), session.draft.as_ref())
        && target != &source.scene_id
    {
        let target = target.clone();
        editor.queue_scene_transition(
            target,
            "正在恢复当前运行场景的编辑标签，上一个场景文档会保留",
        );
    }
}

/// 关闭必须在源文档完整采用、整笔与文件事务收口后执行；取消不回收任何作者数据。
pub(crate) fn finish_scene_document_close(
    mut commands: Commands,
    mut state: SceneDocumentTransition,
    mut editor: ResMut<SceneEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
    global: Res<GlobalEnvironmentAuthoringState>,
    mut workspace: ResMut<VegetationPresetEditorState>,
    mut next_scene: ResMut<NextState<feathrise_scene_core::state::ScenePhase>>,
) {
    if session.water_preset_delete_pending || session.model_vegetation_busy { return; }
    let Some(map) = state
        .documents
        .as_ref()
        .and_then(|docs| docs.close_requested.clone())
    else {
        return;
    };
    // 【功能设计】确认框持有关闭流程，只有按钮选择或保存回执才能推进；等待帧不重判脏位、改写文档或重复诊断。
    if editor.scene_switch_confirmation.is_some()
        || editor.scene_transition_in_progress
        || editor.pending_scene_transition.is_some()
        || session.saving
        || editor.save_requested
        || state.busy()
        || editor.selected_scene_id.as_ref() != Some(&map)
        || session
            .draft
            .as_ref()
            .is_none_or(|draft| draft.scene_id != map)
    {
        return;
    }
    let documents = state.documents.as_mut().unwrap();
    if !documents.close_ready
        && (global.dirty || session.needs_save_before_scene_switch(&editor, &global))
    {
        debug!(scene_id = %map, revision = session.revision, global_dirty = global.dirty, "等待确认关闭未保存场景");
        documents.closing = Some(map.clone());
        editor.scene_switch_confirmation = Some(SceneSwitchConfirmation {
            source_scene_id: Some(map.clone()),
            source_revision: session.revision,
            global_revision: global.revision,
            target_scene_id: map,
            target_display_name: session
                .draft
                .as_ref()
                .unwrap()
                .manifest
                .display_name
                .clone(),
            diagnostic: None,
            stage: SceneSwitchStage::Choosing,
        });
        editor.suppress_world_pick_this_frame = true;
        editor.status = "关闭场景标签前，请选择保存、放弃场景修改或取消".into();
        return;
    }
    let index = documents
        .tabs
        .iter()
        .position(|tab| tab.scene_id == map)
        .unwrap();
    documents.tabs.remove(index);
    let handles = documents.parked_preview_storage.remove(&map).or_else(|| {
        documents
            .preview_storage
            .as_ref()
            .filter(|handles| handles.scene_id == map)
            .is_some()
            .then(|| documents.preview_storage.take().unwrap())
    });
    if let Some(handles) = handles {
        documents
            .retiring_preview_storage
            .insert(map.clone(), handles);
    }
    let closing_scene = map.clone();
    commands.queue(move |world: &mut World| feathrise_scene::close_resident_scene(world, closing_scene)); // 只有显式关闭才回收整个三维驻留场景。
    let next = documents
        .tabs
        .get(index.min(documents.tabs.len().saturating_sub(1)))
        .map(|tab| tab.scene_id.clone());
    documents.close_requested = None;
    documents.closing = None;
    documents.close_ready = false;
    documents.pending_selection = None;
    documents.active_metadata = None;
    let domain_scene = map.clone();
    commands.queue(move |world: &mut World| {
        if let Some(adapter) = world.get_resource::<crate::authoring::domain_documents::SceneDomainAdapter>().copied() { (adapter.close)(world, domain_scene); }
    }); // 具体文档引用租约只在明确关闭后归还。
    if let Some(preview) = state.preview.as_mut() {
        preview.active = None;
    }
    crate::authoring::placement_deletion::withdraw_document(&mut commands, &map, &session.placement_deletions);
    // 明确关闭先撤销文档信用，再后台退役；后台暂持旧session不能让旧删除命令继续取得所有权。
    let undo = std::mem::take(&mut session.undo);
    let redo = std::mem::take(&mut session.redo);
    if let Some(retire) = state.retire.as_mut() {
        retire.push(std::mem::take(&mut *session));
        retire.push(std::mem::take(&mut **state.sparse.as_mut().unwrap()));
        retire.push(std::mem::take(&mut **state.heights.as_mut().unwrap()));
        retire.push(std::mem::take(&mut **state.selection.as_mut().unwrap()));
    }
    editor.selected_scene_id = None;
    editor.dirty = global.dirty;
    workspace.active_tab = PresetWorkspaceTab::Scene;
    workspace.touch_ui();
    if let Some(next) = next {
        session.undo = undo;
        session.redo = redo; // 下一次文档交接在后台仅携带全局历史，关闭的场景命令在那里释放。
        editor.queue_scene_transition(next, "场景标签已关闭，正在恢复相邻场景");
    } else {
        // 最后一个场景文档关闭后回到空工作台；与原--editor入口共用无世界菜单状态。
        state.documents.as_mut().unwrap().cleanup_history = Some(AsyncComputeTaskPool::get().spawn(async move {
            let mut undo = undo;
            let mut redo = redo;
            undo.retain(crate::authoring::EditorCommand::is_global_environment);
            redo.retain(crate::authoring::EditorCommand::is_global_environment);
            (undo, redo)
        }));
        session.saving = true;
        editor.empty_workspace_closed = false; // 关闭最后一个场景是新的空工作台入口，不沿用之前收起F12的意图。
        commands.insert_resource(feathrise_scene_core::state::EditorStartup);
        next_scene.set(feathrise_scene_core::state::ScenePhase::Idle);
        editor.status = "场景标签已关闭，可从目录继续打开场景".into();
    }
}

/// 场景树先完成新的稳定实例索引，再恢复所选对象；新世界中不采用旧Entity身份。
pub(crate) fn restore_scene_document_selection(
    mut documents: ResMut<SceneDocuments>,
    editor: Res<SceneEditorState>,
    mut browser: ResMut<crate::ui::gltf_browser::EditorGltfBrowserState>,
    mut subscenes: Option<ResMut<crate::ui::subscenes::SubsceneEditorState>>,
    mut candidates: Option<ResMut<crate::viewport::LayerCandidateState>>,
    session: Option<Res<crate::authoring::EditorAuthoringSession>>,
    domain: Option<Res<crate::game_authoring::scene_interaction::SceneInteractionState>>,
    modes: Option<Res<crate::viewport::modes::ViewportModeState>>,
) {
    let Some((map, _)) = documents.pending_selection.as_ref() else {
        return;
    };
    if editor.selected_scene_id.as_ref() != Some(map) {
        return;
    }
    if !browser.document_selection_ready(map) {
        return;
    }
    let (_, ui) = documents.pending_selection.take().unwrap();
    if let Some(mut subscenes) = subscenes.take() {
        subscenes.scene_id = editor.selected_scene_id.clone();
        subscenes.selected = ui.subscene.clone();
        if let (Some(id), Some(candidates)) = (ui.subscene.as_ref(), candidates.as_mut()) {
            browser.clear_viewport_selection();
            if let (Some(session), Some(domain), Some(modes)) = (session.as_deref(), domain.as_deref(), modes.as_deref()) {
                candidates.restore_subscene(id, session, domain.observed_generation, modes.selection_filter(&editor));
            }
            return;
        }
    }
    if let Some(mesh) = ui.mesh {
        browser.select_mesh_from_viewport(mesh);
    } else if let Some(placement) = ui.placement {
        browser.select_primitive_placement_from_viewport(placement);
    }
}
