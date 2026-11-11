//! 未保存切换场景的选择、保存续接与一次性放弃授权；文件事务仍由作者域负责。

use feathrise_editor_ui::EditorButtonClick;
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;

use crate::authoring::{EditorAuthoringSession, GlobalEnvironmentAuthoringState};
use crate::components::{SceneEditorActionButton, SceneSwitchDialogBody};
use crate::resources::{SceneBrowserDragState, SceneEditorState};
use crate::types::{EditorAction, SceneSwitchConfirmation, SceneSwitchStage};

mod scene;
pub(crate) use scene::{scene_switch_dialog, sync_scene_switch_dialog};

/// 选择与最终采用都核对同一份源身份；旧授权不能放弃后续编辑或另一个场景。
pub(super) fn source_matches(
    request: &SceneSwitchConfirmation,
    editor: &SceneEditorState,
    session: &EditorAuthoringSession,
    global: &GlobalEnvironmentAuthoringState,
) -> bool {
    request.source_scene_id == editor.selected_scene_id
        && request.source_scene_id.as_ref() == session.draft.as_ref().map(|draft| &draft.scene_id)
        && request.source_revision == session.revision
        && request.global_revision == global.revision
}

fn cancel_switch(editor: &mut SceneEditorState) {
    let was_saving = editor
        .scene_switch_confirmation
        .as_ref()
        .is_some_and(|request| matches!(request.stage, SceneSwitchStage::Saving { .. }));
    if was_saving {
        editor.save_requested = false; // 尚未启动的保存可以取消；已经运行的 IO 继续由原事务安全收口。
    }
    editor.scene_switch_confirmation = None;
    editor.pending_scene_transition = None;
    editor.status = if was_saving {
        "已取消切换场景，当前场景仍保留；已开始的保存会继续完成".to_owned()
    } else {
        "已取消切换场景，继续编辑当前场景".to_owned()
    };
}

/// 模态系统在普通输入门外消费三种按钮；取消的同次按下/松开仍由鼠标门隔离。
pub(crate) fn handle_scene_switch_input(
    mut editor: ResMut<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    global: Res<GlobalEnvironmentAuthoringState>,
    mut drag: ResMut<SceneBrowserDragState>,
    mut keyboard: ResMut<ButtonInput<KeyCode>>,
    buttons: Query<(&EditorButtonClick, &SceneEditorActionButton), Changed<EditorButtonClick>>,
    mut documents: Option<ResMut<crate::ui::scene_documents::SceneDocuments>>,
) {
    if !editor.open || editor.scene_switch_confirmation.is_none() {
        return;
    }
    drag.press = None;
    drag.active = false;
    drag.drop_target = None;
    drag.blocked = true; // 下层作者笔刷及取消后的 release 帧都不能复用这一轮鼠标输入。
    let cancel = keyboard.just_pressed(KeyCode::Escape);
    keyboard.clear(); // 模态期间 Ctrl+S、撤销、删除和 F12 均不能修改背后的草稿。
    if cancel {
        if let Some(documents) = documents.as_mut() {
            documents.close_requested = None;
            documents.closing = None;
            documents.close_ready = false;
        }
        cancel_switch(&mut editor);
        return;
    }
    for (interaction, button) in &buttons {
        if !interaction.is_clicked() {
            continue;
        }
        if button.0 == EditorAction::CancelSceneSwitch {
            if let Some(documents) = documents.as_mut() {
                documents.close_requested = None;
                documents.closing = None;
                documents.close_ready = false;
            }
            cancel_switch(&mut editor);
            return;
        }
        if !matches!(
            button.0,
            EditorAction::SaveAndSwitchScene | EditorAction::DiscardAndSwitchScene
        ) {
            continue;
        }
        let Some(request) = editor.scene_switch_confirmation.clone() else {
            return;
        };
        if request.stage != SceneSwitchStage::Choosing || session.saving || editor.save_requested {
            return; // 等待保存或目标校验时不接受重复保存、放弃操作。
        }
        if !source_matches(&request, &editor, &session, &global) {
            if request.source_scene_id != editor.selected_scene_id
                || request.source_scene_id.as_ref()
                    != session.draft.as_ref().map(|draft| &draft.scene_id)
            {
                if let Some(documents) = documents.as_mut() {
                    documents.close_requested = None;
                    documents.closing = None;
                    documents.close_ready = false;
                }
                cancel_switch(&mut editor);
                editor.status = "当前场景会话已变化；已取消旧切换场景请求，草稿仍保留".to_owned();
            } else {
                let current = editor.scene_switch_confirmation.as_mut().unwrap();
                current.source_revision = session.revision;
                current.global_revision = global.revision;
                current.diagnostic =
                    Some("当前草稿已更新；请重新选择如何处理这些未保存修改".to_owned());
                editor.status = "当前草稿已更新；请重新选择如何处理这些未保存修改".to_owned();
            }
            return; // 更新提示不等于新授权，必须再次明确点击。
        }
        let stage = if button.0 == EditorAction::SaveAndSwitchScene {
            editor.save_requested = true;
            editor.status = if documents
                .as_ref()
                .is_some_and(|docs| docs.closing.is_some())
            {
                "正在保存当前修改；成功后关闭场景标签，可取消关闭"
            } else {
                "正在保存当前修改；成功后自动进入目标场景，可取消切换场景"
            }
            .to_owned();
            SceneSwitchStage::Saving {
                success_sequence_before: session.successful_save_sequence,
            }
        } else {
            if let Some(documents) = documents.as_mut()
                && documents.closing.as_ref() == request.source_scene_id.as_ref()
            {
                documents.close_ready = true;
                editor.scene_switch_confirmation = None;
                editor.suppress_world_pick_this_frame = true;
                return; // 明确放弃只关闭这个场景；全局环境和其他标签的草稿仍由工作台持有。
            }
            editor.queue_scene_transition(
                request.target_scene_id,
                "正在校验目标场景；校验通过后放弃当前未保存修改并切换",
            ); // 此处只登记授权；目标缺失或清单错误时仍完整保留当前草稿。
            SceneSwitchStage::DiscardApproved
        };
        let current = editor.scene_switch_confirmation.as_mut().unwrap();
        current.stage = stage;
        current.diagnostic = None;
        return;
    }
}

/// 位于三种既有保存轮询之后，成功回执、源身份和全部脏位同时匹配才续接切换场景。
pub(crate) fn resume_scene_switch_after_save(
    mut editor: ResMut<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    global: Res<GlobalEnvironmentAuthoringState>,
    mut documents: Option<ResMut<crate::ui::scene_documents::SceneDocuments>>,
) {
    let Some(request) = editor.scene_switch_confirmation.as_ref() else {
        return;
    };
    let SceneSwitchStage::Saving {
        success_sequence_before,
    } = request.stage
    else {
        return;
    };
    if session.saving || editor.save_requested {
        return; // 预检、稀疏块和联合保存的各阶段都保持模态；不额外建立后台任务。
    }
    if !source_matches(request, &editor, &session, &global)
        || !editor.open
        || editor.close_requested
    {
        if let Some(documents) = documents.as_mut() {
            documents.close_requested = None;
            documents.closing = None;
            documents.close_ready = false;
        }
        cancel_switch(&mut editor);
        editor.status =
            "保存期间场景会话或作者修订已变化；已取消自动切换场景，当前草稿仍保留".to_owned();
        return;
    }
    if session.successful_save_sequence != success_sequence_before && !editor.dirty && !global.dirty
    {
        let request = editor.scene_switch_confirmation.take().unwrap();
        if let Some(documents) = documents.as_mut()
            && documents.closing.as_ref() == request.source_scene_id.as_ref()
        {
            documents.close_ready = true;
            return; // 仅完整保存成功回执可继续关闭，失败仍在三选一模态内保留草稿。
        }
        editor.queue_scene_transition(
            request.target_scene_id,
            format!(
                "保存成功，正在准备进入场景“{}”",
                request.target_display_name
            ),
        ); // 下一帧仍经过正式清单与笔划检查，不能从保存系统旁路直接切换场景。
    } else {
        let action = if documents
            .as_ref()
            .is_some_and(|docs| docs.closing.is_some())
        {
            "保存并关闭"
        } else {
            "保存并切换场景"
        };
        let message = format!("未能完成{action}：{}；当前修改仍保留", editor.status);
        editor.scene_switch_confirmation.as_mut().unwrap().stage = SceneSwitchStage::Choosing;
        editor.scene_switch_confirmation.as_mut().unwrap().diagnostic = Some(message.clone());
        editor.status = message;
    }
}

/// 模态正文滚轮由独立入口消费，底层生产栏与场景相机继续服从模态输入门。
pub(crate) fn scroll_scene_switch_body(
    mut wheels: MessageReader<MouseWheel>,
    editor: Res<SceneEditorState>,
    mut bodies: Query<
        (&RelativeCursorPosition, &ComputedNode, &mut ScrollPosition),
        With<SceneSwitchDialogBody>,
    >,
) {
    let delta = wheels
        .read()
        .map(|event| {
            -event.y
                * match event.unit {
                    MouseScrollUnit::Line => 27.0, // 沿用生产栏的一行滚动距离。
                    MouseScrollUnit::Pixel => 1.0,
                }
        })
        .sum::<f32>();
    if !editor.open || editor.scene_switch_confirmation.is_none() || delta == 0.0 {
        return;
    }
    for (cursor, node, mut scroll) in &mut bodies {
        if cursor.cursor_over() {
            let maximum =
                ((node.content_size().y - node.size().y) * node.inverse_scale_factor()).max(0.0);
            scroll.y = (scroll.y + delta).clamp(0.0, maximum);
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod layout_tests;
