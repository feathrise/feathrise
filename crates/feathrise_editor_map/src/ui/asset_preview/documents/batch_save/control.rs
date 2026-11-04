//! 批量受理冻结当前草稿；回执只结清提交保存点，继续输入与失败文件都留在原会话。

use super::super::{
    components::DocumentAction,
    constants::MAX_DOCUMENTS,
    resources::DocumentState,
    search::{SearchGate, busy},
    types::IoResult,
};
use super::{SaveAllDocuments, components::*, resources::DocumentBatchSaveState, types::*, worker};
use crate::{
    resources::MapEditorState,
    ui::vegetation_preset_editor::{PresetWorkspaceTab, VegetationPresetEditorState},
};
use bevy::{
    input::mouse::{MouseScrollUnit, MouseWheel},
    prelude::*,
    tasks::IoTaskPool,
    text::EditableText,
    ui::RelativeCursorPosition,
};

pub(in super::super) fn request(
    state: &mut DocumentBatchSaveState,
    documents: &mut DocumentState,
    scope: SaveScope,
    content: feathrise_editor_documents_model::DocumentContentOperations,
) {
    state.shown = true;
    if documents.io.is_some()
        || documents.pending_load.is_some()
        || super::super::json::save_busy(documents)
    {
        state.hint = "文件保存、载入或JSON格式化进行中，请完成后再保存".into();
        return; // 单飞背压：保留当前结果与在途票据，不积压用户动作。
    }
    let (label, keys) = match scope {
        SaveScope::All => (
            "全部草稿",
            documents
                .documents
                .iter()
                .filter(|doc| doc.dirty())
                .map(|doc| doc.source_key())
                .collect::<Vec<_>>(),
        ),
        SaveScope::Batch(keys) => ("最近替换本批的当前草稿", keys),
    };
    if keys.len() > MAX_DOCUMENTS {
        state.hint = "一次最多保存8份文档，范围超限，未启动保存".into();
        return;
    }
    state.ticket = state.ticket.wrapping_add(1);
    state.scope = label.into();
    state.files.clear();
    state.reset_scroll = true;
    let mut inputs = Vec::with_capacity(keys.len());
    for key in keys {
        if state.files.iter().any(|file| file.path == key.path) {
            continue;
        }
        let index = state.files.len();
        let mut report = FileReport {
            path: super::super::source::display_path(&key.path).into(),
            outcome: FileOutcome::Pending,
            detail: "正在保存点击时的草稿…".into(),
        };
        match documents
            .documents
            .iter_mut()
            .find(|doc| doc.path == key.path && doc.generation == key.generation)
        {
            Some(doc) if doc.dirty() => {
                doc.confirm_reload = false;
                doc.history.break_group(); // 后续连续输入必须另成一步，不能藏在提交保存点内。
                doc.status = report.detail.clone();
                inputs.push(SaveInput {
                    index,
                    key,
                    revision: doc.content_revision(),
                    disk: doc.disk.clone(),
                    pages: doc.pages.clone(),
                    source: doc.source.clone(),
                });
            }
            Some(_) => {
                report.outcome = FileOutcome::Unmodified;
                report.detail = "没有未保存改动".into();
            }
            None => {
                report.outcome = FileOutcome::Failed;
                report.detail = "本批文档已重载或离开会话，请确认当前文件后单独保存".into();
            }
        }
        state.files.push(report);
    }
    state.running = !inputs.is_empty();
    state.hint = if state.running {
        "后台逐文件校验和保存，失败文件保留草稿"
    } else {
        "此范围没有可保存的修改"
    }
    .into();
    if inputs.is_empty() {
        return;
    }
    let roots = documents.source_roots();
    let ticket = state.ticket;
    documents.saving_batch = inputs.iter().map(|input| input.key.clone()).collect();
    documents.saving = inputs.first().map(|input| input.key.path.clone());
    documents.io = Some(
        IoTaskPool::get()
            .spawn(async move { IoResult::SavedBatch(worker::save_sources(ticket, roots, inputs, content).await) }),
    );
}

pub(in super::super) fn adopt(
    documents: &mut DocumentState,
    state: Option<&mut DocumentBatchSaveState>,
    reply: SaveReply,
) {
    let mut state = state.filter(|state| state.ticket == reply.ticket);
    for receipt in reply.files {
        let doc = documents.documents.iter_mut().find(|doc| {
            documents.root == reply.root
                && doc.path == receipt.key.path
                && doc.generation == receipt.key.generation
        });
        let (outcome, detail) = match receipt.result {
            Ok(disk) => {
                let detail = if let Some(doc) = doc {
                    doc.disk = disk;
                    doc.saved_revision = receipt.revision;
                    doc.external = default();
                    let text = if doc.dirty() {
                        "已保存点击时的草稿；之后的修改仍未保存"
                    } else {
                        "已保存完整文档"
                    };
                    doc.status = text.into();
                    text
                } else {
                    "已保存原文件；当前不同代次的会话保持"
                };
                (FileOutcome::Saved, detail.into())
            }
            Err(error) => {
                if let Some(doc) = doc {
                    doc.status = error.clone();
                }
                (FileOutcome::Failed, error)
            }
        };
        if let Some(state) = state.as_deref_mut()
            && let Some(report) = state
                .files
                .get_mut(receipt.index)
                .filter(|file| file.path == receipt.key.path)
        {
            report.outcome = outcome;
            report.detail = detail;
        }
    }
    if let Some(state) = state {
        state.running = false;
        state.hint = if state
            .files
            .iter()
            .any(|file| file.outcome == FileOutcome::Failed)
        {
            "保存已完成；失败项保留草稿，修正后可重试"
        } else {
            "保存已完成；已写入点击时的草稿快照"
        }
        .into();
    }
}

pub(super) fn input(
    editor: Res<MapEditorState>,
    content: Res<feathrise_editor_documents_model::DocumentContentOperations>,
    gate: SearchGate,
    workspace: Res<VegetationPresetEditorState>,
    replacement: Option<Res<super::super::search::AssetReplaceState>>,
    mut documents: ResMut<DocumentState>,
    mut state: ResMut<DocumentBatchSaveState>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut menu_requests: MessageReader<SaveAllDocuments>,
    sources: Query<&EditableText>,
    buttons: Query<(&DocumentAction, &feathrise_editor_ui::EditorButtonClick), Changed<feathrise_editor_ui::EditorButtonClick>>,
    hide: Query<&feathrise_editor_ui::EditorButtonClick, (With<HideSaveReport>, Changed<feathrise_editor_ui::EditorButtonClick>)>,
) {
    let menu_requested = menu_requests.read().count() > 0;
    let shown = documents.visible
        || replacement
            .as_ref()
            .is_some_and(|state| state.preview_visible())
            && workspace.active_tab == PresetWorkspaceTab::Document;
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let save_key = shown
        && ctrl
        && shift
        && keys.just_pressed(KeyCode::KeyS)
        && !keys.pressed(KeyCode::AltLeft)
        && !keys.pressed(KeyCode::AltRight);
    if save_key {
        keys.clear_just_pressed(KeyCode::KeyS);
    } // 被拒绝时也不能穿透到单文件或地图保存。
    let pressed = shown
        && buttons.iter().any(|(action, interaction)| {
            *action == DocumentAction::SaveAll && interaction.is_clicked()
        });
    // 菜单消息已由菜单自己的完整模态/IME门受检；同帧菜单仍捕获鼠标，不再以该捕获拦截意图。
    if !editor.open || editor.close_requested || (!menu_requested && gate.blocked(&editor)) {
        return;
    }
    if hide
        .iter()
        .any(|interaction| interaction.is_clicked())
    {
        state.shown = false;
    }
    if !menu_requested && !pressed && !save_key {
        return;
    }
    if sources.iter().any(busy) {
        state.shown = true;
        state.hint = "请先确认中文输入或等待粘贴完成，再保存草稿".into();
        return;
    }
    request(&mut state, &mut documents, SaveScope::All, *content);
}

pub(super) fn scroll(
    mut wheels: MessageReader<MouseWheel>,
    editor: Res<MapEditorState>,
    gate: SearchGate,
    mut state: ResMut<DocumentBatchSaveState>,
    mut views: Query<
        (&RelativeCursorPosition, &ComputedNode, &mut ScrollPosition),
        With<SaveReportScroll>,
    >,
) {
    if state.wheel_owned {
        state.wheel_owned = false;
    }
    let delta: f32 = wheels
        .read()
        .map(|wheel| {
            -wheel.y
                * if wheel.unit == MouseScrollUnit::Line {
                    27.0
                } else {
                    1.0
                }
        })
        .sum();
    if !state.shown || gate.blocked(&editor) {
        return;
    }
    for (cursor, node, mut scroll) in &mut views {
        if state.reset_scroll {
            scroll.y = 0.0;
        }
        if delta != 0.0 && cursor.cursor_over() && node.size().y > 0.0 {
            let max =
                ((node.content_size().y - node.size().y) * node.inverse_scale_factor()).max(0.0);
            scroll.y = (scroll.y + delta).clamp(0.0, max);
            state.wheel_owned = true; // 边缘也独占，不能滚动外层资产或地图。
        }
    }
    if state.reset_scroll {
        state.reset_scroll = false;
    }
}
