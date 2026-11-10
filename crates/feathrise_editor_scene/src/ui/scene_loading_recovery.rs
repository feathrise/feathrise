//! 加载恢复只撤销运行时请求；草稿与历史保留，场景按原驻留预算退役后再冷重试。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::EditorButtonClick;
use bevy::prelude::*;
use feathrise_scene_core::state::{ScenePhase, LoadingProgress, LoadingStage, PendingSceneTransition};
use crate::authoring::{DraftLoadTask, EditorAuthoringSession};
use crate::components::SceneLoadingRecoveryButton;
use crate::resources::{SceneEditorState, SceneLoadingRecovery, SceneLoadingRecoveryRequest};
use super::scene_documents::SceneDocuments;

pub(crate) fn handle_recovery_buttons(
    buttons: Query<(&EditorButtonClick, &SceneLoadingRecoveryButton), Changed<EditorButtonClick>>,
    mut editor: ResMut<SceneEditorState>,
    progress: Option<Res<LoadingProgress>>,
    mut recovery: ResMut<SceneLoadingRecovery>,
) {
    if !editor.open || !editor.scene_transition_in_progress || recovery.request.is_some() { return; }
    for (interaction, action) in &buttons {
        if !interaction.is_clicked() { continue; }
        let retry = *action == SceneLoadingRecoveryButton::Retry;
        if retry && progress.as_ref().is_none_or(|value| value.stage != LoadingStage::Failed) { continue; }
        let Some(scene_id) = editor.pending_scene_transition.clone().or_else(|| editor.selected_scene_id.clone()) else { return; };
        recovery.request = Some(SceneLoadingRecoveryRequest { scene_id, retry, returning: false });
        editor.close_requested = false;
        editor.suppress_world_pick_this_frame = true;
        editor.status = "正在收口场景文档交接；草稿与其他标签会保留".into();
        return;
    }
}

pub(crate) fn sync_recovery_buttons(
    editor: Res<SceneEditorState>,
    progress: Option<Res<LoadingProgress>>,
    recovery: Res<SceneLoadingRecovery>,
    mut buttons: Query<(&SceneLoadingRecoveryButton, &mut Node, &mut UiInteraction)>,
) {
    for (action, mut node, mut interaction) in &mut buttons {
        let visible = editor.open && editor.scene_transition_in_progress && recovery.request.is_none()
            && (*action == SceneLoadingRecoveryButton::Cancel
                || progress.as_ref().is_some_and(|value| value.stage == LoadingStage::Failed));
        let display = if visible { Display::Flex } else { Display::None };
        if node.display != display { node.display = display; }
        if !visible && *interaction != UiInteraction::None { *interaction = UiInteraction::None; }
    }
}

/// 在Last裁决，避免同帧LoadingScene轮询覆盖返回请求；等待期间不阻塞任何后台任务。
pub(crate) fn advance_recovery(world: &mut World) {
    let Some(request) = world.resource::<SceneLoadingRecovery>().request.as_ref() else { return; };
    let map = request.scene_id.clone();
    let retry = request.retry;
    let returning = request.returning;
    let switching = world.get_resource::<feathrise_scene_core::residency::SceneResidencyActivity>()
        .is_some_and(|activity| activity.switching);
    if returning {
        if *world.resource::<State<ScenePhase>>().get() != ScenePhase::Idle || switching
            || world.get_resource::<feathrise_scene::ResidentScenes>().is_some_and(|scenes| scenes.contains(&map)) { return; }
        {
            let mut recovery = world.resource_mut::<SceneLoadingRecovery>();
            recovery.request = None;
            recovery.epoch = recovery.epoch.wrapping_add(1);
        }
        let mut editor = world.resource_mut::<SceneEditorState>();
        editor.scene_transition_in_progress = false;
        editor.status = "已取消场景加载，草稿和其他标签已保留，可重新打开场景".into();
        if retry { editor.queue_scene_transition(map, "正在重新加载场景"); }
        return;
    }
    let documents = world.resource::<SceneDocuments>();
    if switching || documents.preparing_target.is_some() || documents.history_task.is_some()
        || world.resource::<SceneEditorState>().pending_scene_transition.is_some() { return; }
    // 缓存文档由既有采用链先恢复，绝不能丢弃其中未保存的场景历史。
    if documents.parked.contains_key(&map) { return; }
    if world.resource::<SceneEditorState>().selected_scene_id.as_ref() != Some(&map) {
        world.resource_mut::<SceneLoadingRecovery>().request = None;
        return; // 目标已换代，旧恢复请求不得取消后来打开的场景。
    }
    if let Some(adapter) = world.get_resource::<crate::authoring::domain_documents::SceneDomainAdapter>().copied() && !(adapter.recover)(world) { return; }
    if let Some(history) = world.resource_mut::<SceneDocuments>().pending_history.take() {
        // 冷载尚无作者草稿时只余全局历史；原场景历史已被poll_scene_document_history归还原标签。
        let mut session = world.resource_mut::<EditorAuthoringSession>();
        session.undo = history.target_undo;
        session.redo = history.target_redo;
    }
    {
        let mut load = world.resource_mut::<DraftLoadTask>();
        load.desired_scene_id = None;
        load.ticket = load.ticket.wrapping_add(1); // 保留在途IO槽，迟到结果只能后台回收。
    }
    world.remove_resource::<PendingSceneTransition>();
    {
        let mut documents = world.resource_mut::<SceneDocuments>();
        if documents.preview_storage.as_ref().is_some_and(|handles| handles.scene_id == map)
            && let Some(handles) = documents.preview_storage.take()
        {
            documents.retiring_preview_storage.insert(map.clone(), handles);
        } // 与场景原子断开后再后台释放私有预览资产，不让取消后的冷重试积累旧强句柄。
    }
    feathrise_scene::close_resident_scene(world, map); // 只退役本次目标的运行时，作者数据继续由文档持有。
    world.insert_resource(feathrise_scene_core::state::EditorStartup);
    world.resource_mut::<NextState<ScenePhase>>().set(ScenePhase::Idle);
    world.resource_mut::<SceneLoadingRecovery>().request.as_mut().unwrap().returning = true;
    let mut editor = world.resource_mut::<SceneEditorState>();
    editor.selected_scene_id = None;
    editor.empty_workspace_closed = false;
    editor.status = "正在返回空工作台并回收未完成场景；草稿与其他标签已保留".into();
}

#[cfg(test)]
#[path = "scene_loading_recovery_tests.rs"]
mod tests;
