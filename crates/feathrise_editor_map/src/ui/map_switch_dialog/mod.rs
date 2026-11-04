//! 未保存切图的选择、保存续接与一次性放弃授权；文件事务仍由作者域负责。

use feathrise_editor_ui::EditorButtonClick;
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;

use crate::authoring::{EditorAuthoringSession, GlobalEnvironmentAuthoringState};
use crate::components::{MapEditorActionButton, MapSwitchDialogBody};
use crate::resources::{MapBrowserDragState, MapEditorState};
use crate::types::{EditorAction, MapSwitchConfirmation, MapSwitchStage};

mod scene;
pub(crate) use scene::{map_switch_dialog, sync_map_switch_dialog};

/// 选择与最终采用都核对同一份源身份；旧授权不能放弃后续编辑或另一张地图。
pub(super) fn source_matches(
    request: &MapSwitchConfirmation,
    editor: &MapEditorState,
    session: &EditorAuthoringSession,
    global: &GlobalEnvironmentAuthoringState,
) -> bool {
    request.source_map_id == editor.selected_map_id
        && request.source_map_id.as_ref() == session.draft.as_ref().map(|draft| &draft.map_id)
        && request.source_revision == session.revision
        && request.global_revision == global.revision
}

fn cancel_switch(editor: &mut MapEditorState) {
    let was_saving = editor
        .map_switch_confirmation
        .as_ref()
        .is_some_and(|request| matches!(request.stage, MapSwitchStage::Saving { .. }));
    if was_saving {
        editor.save_requested = false; // 尚未启动的保存可以取消；已经运行的 IO 继续由原事务安全收口。
    }
    editor.map_switch_confirmation = None;
    editor.pending_map_transition = None;
    editor.status = if was_saving {
        "已取消切图，当前地图仍保留；已开始的保存会继续完成".to_owned()
    } else {
        "已取消切图，继续编辑当前地图".to_owned()
    };
}

/// 模态系统在普通输入门外消费三种按钮；取消的同次按下/松开仍由鼠标门隔离。
pub(crate) fn handle_map_switch_input(
    mut editor: ResMut<MapEditorState>,
    session: Res<EditorAuthoringSession>,
    global: Res<GlobalEnvironmentAuthoringState>,
    mut drag: ResMut<MapBrowserDragState>,
    mut keyboard: ResMut<ButtonInput<KeyCode>>,
    buttons: Query<(&EditorButtonClick, &MapEditorActionButton), Changed<EditorButtonClick>>,
    mut documents: Option<ResMut<crate::ui::map_documents::MapDocuments>>,
) {
    if !editor.open || editor.map_switch_confirmation.is_none() {
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
        if button.0 == EditorAction::CancelMapSwitch {
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
            EditorAction::SaveAndSwitchMap | EditorAction::DiscardAndSwitchMap
        ) {
            continue;
        }
        let Some(request) = editor.map_switch_confirmation.clone() else {
            return;
        };
        if request.stage != MapSwitchStage::Choosing || session.saving || editor.save_requested {
            return; // 等待保存或目标校验时不接受重复保存、放弃操作。
        }
        if !source_matches(&request, &editor, &session, &global) {
            if request.source_map_id != editor.selected_map_id
                || request.source_map_id.as_ref()
                    != session.draft.as_ref().map(|draft| &draft.map_id)
            {
                if let Some(documents) = documents.as_mut() {
                    documents.close_requested = None;
                    documents.closing = None;
                    documents.close_ready = false;
                }
                cancel_switch(&mut editor);
                editor.status = "当前地图会话已变化；已取消旧切图请求，草稿仍保留".to_owned();
            } else {
                let current = editor.map_switch_confirmation.as_mut().unwrap();
                current.source_revision = session.revision;
                current.global_revision = global.revision;
                current.diagnostic =
                    Some("当前草稿已更新；请重新选择如何处理这些未保存修改".to_owned());
                editor.status = "当前草稿已更新；请重新选择如何处理这些未保存修改".to_owned();
            }
            return; // 更新提示不等于新授权，必须再次明确点击。
        }
        let stage = if button.0 == EditorAction::SaveAndSwitchMap {
            editor.save_requested = true;
            editor.status = if documents
                .as_ref()
                .is_some_and(|docs| docs.closing.is_some())
            {
                "正在保存当前修改；成功后关闭地图标签，可取消关闭"
            } else {
                "正在保存当前修改；成功后自动进入目标地图，可取消切图"
            }
            .to_owned();
            MapSwitchStage::Saving {
                success_sequence_before: session.successful_save_sequence,
            }
        } else {
            if let Some(documents) = documents.as_mut()
                && documents.closing.as_ref() == request.source_map_id.as_ref()
            {
                documents.close_ready = true;
                editor.map_switch_confirmation = None;
                editor.suppress_world_pick_this_frame = true;
                return; // 明确放弃只关闭这张地图；全局环境和其他标签的草稿仍由工作台持有。
            }
            editor.queue_map_transition(
                request.target_map_id,
                "正在校验目标地图；校验通过后放弃当前未保存修改并切换",
            ); // 此处只登记授权；目标缺失或清单错误时仍完整保留当前草稿。
            MapSwitchStage::DiscardApproved
        };
        let current = editor.map_switch_confirmation.as_mut().unwrap();
        current.stage = stage;
        current.diagnostic = None;
        return;
    }
}

/// 位于三种既有保存轮询之后，成功回执、源身份和全部脏位同时匹配才续接切图。
pub(crate) fn resume_map_switch_after_save(
    mut editor: ResMut<MapEditorState>,
    session: Res<EditorAuthoringSession>,
    global: Res<GlobalEnvironmentAuthoringState>,
    mut documents: Option<ResMut<crate::ui::map_documents::MapDocuments>>,
) {
    let Some(request) = editor.map_switch_confirmation.as_ref() else {
        return;
    };
    let MapSwitchStage::Saving {
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
            "保存期间地图会话或作者修订已变化；已取消自动切图，当前草稿仍保留".to_owned();
        return;
    }
    if session.successful_save_sequence != success_sequence_before && !editor.dirty && !global.dirty
    {
        let request = editor.map_switch_confirmation.take().unwrap();
        if let Some(documents) = documents.as_mut()
            && documents.closing.as_ref() == request.source_map_id.as_ref()
        {
            documents.close_ready = true;
            return; // 仅完整保存成功回执可继续关闭，失败仍在三选一模态内保留草稿。
        }
        editor.queue_map_transition(
            request.target_map_id,
            format!(
                "保存成功，正在准备进入地图“{}”",
                request.target_display_name
            ),
        ); // 下一帧仍经过正式清单与笔划检查，不能从保存系统旁路直接换图。
    } else {
        let action = if documents
            .as_ref()
            .is_some_and(|docs| docs.closing.is_some())
        {
            "保存并关闭"
        } else {
            "保存并切图"
        };
        let message = format!("未能完成{action}：{}；当前修改仍保留", editor.status);
        editor.map_switch_confirmation.as_mut().unwrap().stage = MapSwitchStage::Choosing;
        editor.map_switch_confirmation.as_mut().unwrap().diagnostic = Some(message.clone());
        editor.status = message;
    }
}

/// 模态正文滚轮由独立入口消费，底层生产栏与地图相机继续服从模态输入门。
pub(crate) fn scroll_map_switch_body(
    mut wheels: MessageReader<MouseWheel>,
    editor: Res<MapEditorState>,
    mut bodies: Query<
        (&RelativeCursorPosition, &ComputedNode, &mut ScrollPosition),
        With<MapSwitchDialogBody>,
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
    if !editor.open || editor.map_switch_confirmation.is_none() || delta == 0.0 {
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
