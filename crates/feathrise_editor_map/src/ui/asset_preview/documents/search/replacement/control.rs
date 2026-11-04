//! 替换表单与中央预览操作；候选、文件任务及模态不能触发草稿变换，Enter只生成预览。

use super::super::super::{
    components::{DocumentAction, DocumentSource},
    constants::MAX_DOCUMENTS,
    history::Selection,
    resources::{DocumentInputs, DocumentState},
    types::ViewMode,
};
use super::super::{
    control::{SearchGate, busy},
    resources::AssetSearchState,
    types::DraftStamp,
};
use super::{components::*, constants::*, resources::AssetReplaceState, tasks, types::*};
use crate::{
    resources::*,
    ui::{
        sidebars::MapEditorSidebarState,
        vegetation_preset_editor::{PresetWorkspaceTab, VegetationPresetEditorState},
    },
};
use bevy::{
    input::mouse::{MouseScrollUnit, MouseWheel},
    input_focus::InputFocus,
    prelude::*,
    text::EditableText,
    ui::RelativeCursorPosition,
};

fn generate(
    state: &mut AssetReplaceState,
    search: &AssetSearchState,
    documents: &DocumentState,
    inputs: &DocumentInputs,
    sources: &Query<(&DocumentSource, &EditableText)>,
) {
    let Some(result) = search
        .result
        .as_ref()
        .filter(|result| !result.references && search.current().is_some() && Some(result.ticket) == state.search_ticket)
    else {
        state.hint = "搜索结果已失效，请重新搜索并打开替换预览".into();
        return;
    };
    if state.checked.is_empty() {
        state.hint = "请先勾选需要预览的文件，最多8个".into();
        return;
    }
    let drafts = documents
        .documents
        .iter()
        .filter(|doc| {
            state
                .checked
                .iter()
                .any(|index| result.files[*index].path == doc.path)
        })
        .map(|doc| {
            let key = doc.source_key();
            let selection = sources
                .iter()
                .find(|(binding, _)| binding.key.as_ref() == Some(&key))
                .map(|(_, input)| Selection::read(input))
                .or_else(|| {
                    inputs
                        .sessions
                        .iter()
                        .find(|session| session.key == key && session.revision == doc.revision)
                        .map(|session| Selection::read(&session.input))
                })
                .unwrap_or_default();
            DraftInput {
                stamp: DraftStamp {
                    key,
                    revision: doc.revision,
                },
                disk: doc.disk.clone(),
                source: doc.source.clone(),
                pages: doc.pages.clone(),
                selection,
            }
        })
        .collect();
    state.request(Work::Preview {
        search: result.clone(),
        query: search.query.clone(),
        replacement: state.replacement.clone(),
        case_sensitive: search.case_sensitive,
        whole_word: search.whole_word,
        checked: state.checked.clone(),
        drafts,
    });
}

pub(super) fn input(
    mut editor: ResMut<MapEditorState>,
    sidebar: Res<MapEditorSidebarState>,
    gate: SearchGate,
    search: Res<AssetSearchState>,
    mut state: ResMut<AssetReplaceState>,
    mut documents: ResMut<DocumentState>,
    inputs: Res<DocumentInputs>,
    mut workspace: ResMut<VegetationPresetEditorState>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut focus: ResMut<InputFocus>,
    fields: Query<(Entity, &EditableText), With<ReplaceField>>,
    all_inputs: Query<&EditableText>,
    sources: Query<(&DocumentSource, &EditableText)>,
    buttons: Query<(&ReplaceAction, &feathrise_editor_ui::EditorButtonClick), Changed<feathrise_editor_ui::EditorButtonClick>>,
    document_buttons: Query<(&DocumentAction, &feathrise_editor_ui::EditorButtonClick), Changed<feathrise_editor_ui::EditorButtonClick>>,
    (mut saving, content): (
        Option<ResMut<super::super::super::batch_save::DocumentBatchSaveState>>,
        Res<feathrise_editor_documents_model::DocumentContentOperations>,
    ),
) {
    if search.path_query() {
        // 引用结果的范围包含完整JSON字符串，只能导航，禁止旧按钮或Ctrl+Shift+H把它用于替换。
        // 原批次历史继续保留；在途工作由Tasks按搜索票据取消，不在输入帧释放大快照。
        if state.active { state.active = false; }
        let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
        let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
        if !gate.blocked(&editor) && !all_inputs.iter().any(busy)
            && ctrl && shift && keys.just_pressed(KeyCode::KeyH)
        {
            keys.clear_just_pressed(KeyCode::KeyH); // 拒绝跨文件替换时也不穿透成当前文件Ctrl+H。
        }
        return;
    }
    // 标签激活、打开文件和展开切换会退出替换预览，统一使用现有导航入口。
    if state.active
        && (workspace.active_tab != PresetWorkspaceTab::Document
            || documents.center_activation != state.center_activation
            || document_buttons.iter().any(|(action, interaction)| {
                interaction.is_clicked()
                    && matches!(
                        action,
                        DocumentAction::ActivateTab(_)
                            | DocumentAction::OpenSelected
                            | DocumentAction::ToggleExpand
                    )
            }))
    {
        state.active = false;
    }
    if gate.blocked(&editor) || all_inputs.iter().any(busy) {
        return;
    }
    if sidebar.assets_browser_visible() && search.open {
        for (_, field) in &fields {
            if field.value() != state.replacement.as_str() {
                state.replacement = field.value().to_string();
                state.cancel("替换文字已变化，请更新预览；草稿保持");
            }
        }
    }
    let mut actions: Vec<_> = buttons
        .iter()
        .filter_map(|(action, interaction)| {
            (interaction.is_clicked()).then_some(*action)
        })
        .collect();
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    if ctrl && shift && keys.just_pressed(KeyCode::KeyH) {
        actions.push(ReplaceAction::Open);
        keys.clear_just_pressed(KeyCode::KeyH); // 不穿透为当前文件Ctrl+H。
    }
    if focus.get().is_some_and(|entity| fields.contains(entity))
        && keys.just_pressed(KeyCode::Enter)
    {
        actions.push(if state.preview_visible() {
            ReplaceAction::Generate
        } else {
            ReplaceAction::Open
        });
        keys.clear_just_pressed(KeyCode::Enter);
    }
    if state.preview_visible() && keys.just_pressed(KeyCode::Escape) {
        actions.push(ReplaceAction::Close);
        keys.clear_just_pressed(KeyCode::Escape);
    }
    for action in actions {
        match action {
            ReplaceAction::Open => {
                let Some(result) = search
                    .current()
                    .filter(|result| !result.references && result.hits > 0 && result.error.is_empty())
                else {
                    state.hint = "请先完成内容搜索，再打开替换预览".into();
                    continue;
                };
                if workspace.active_tab != PresetWorkspaceTab::Document {
                    state.return_tab = workspace.active_tab;
                }
                state.search_ticket = Some(result.ticket);
                state.checked = if result.files.len() <= MAX_DOCUMENTS {
                    (0..result.files.len()).collect()
                } else {
                    Vec::new()
                };
                state.file = 0;
                state.hit = 0;
                state.tab_open = true;
                state.active = true;
                state.center_activation = documents.center_activation;
                state.reset_files = true;
                state.reset_diff = true;
                workspace.active_tab = PresetWorkspaceTab::Document;
                workspace.touch_ui();
                focus.clear();
                if state.checked.is_empty() {
                    state.cancel("匹配文件超过8个，请勾选本批文件后更新预览；不会截断或自动应用");
                } else {
                    generate(&mut state, &search, &documents, &inputs, &sources);
                }
            }
            ReplaceAction::Activate if state.tab_open => {
                state.active = true;
                workspace.active_tab = PresetWorkspaceTab::Document;
                workspace.touch_ui();
                focus.clear();
                state.center_activation = documents.center_activation;
            }
            ReplaceAction::Close if state.tab_open => {
                state.cancel("替换预览已关闭，草稿保持");
                state.tab_open = false;
                state.active = false;
                if workspace.active_tab == PresetWorkspaceTab::Document
                    && documents.expanded.is_none()
                {
                    workspace.active_tab = state.return_tab;
                    workspace.touch_ui();
                }
                focus.clear();
            }
            ReplaceAction::Generate if state.preview_visible() => {
                generate(&mut state, &search, &documents, &inputs, &sources)
            }
            ReplaceAction::SelectAll if state.preview_visible() => {
                if let Some(result) = search
                    .current()
                    .filter(|result| Some(result.ticket) == state.search_ticket)
                {
                    if result.files.len() > MAX_DOCUMENTS {
                        state.hint = "一次最多勾选8个文件，请分批选择；未自动截断".into();
                    } else {
                        state.checked = (0..result.files.len()).collect();
                        state.cancel("勾选已变化，请更新预览");
                    }
                }
            }
            ReplaceAction::SelectNone if state.preview_visible() => {
                state.checked.clear();
                state.cancel("勾选已清空，请选择文件并更新预览");
            }
            ReplaceAction::Check { ticket, index }
                if ticket == state.preview_ticket && state.preview_visible() && !state.applied =>
            {
                if let Some(at) = state.checked.iter().position(|file| *file == index) {
                    state.checked.remove(at);
                } else if state.checked.len() < MAX_DOCUMENTS {
                    state.checked.push(index);
                    state.checked.sort_unstable();
                } else {
                    state.hint = "一次最多勾选8个文件，请先取消一个文件".into();
                    continue;
                }
                state.cancel("勾选已变化，请更新预览");
            }
            ReplaceAction::File { ticket, index }
                if ticket == state.preview_ticket && state.preview_visible() =>
            {
                state.file = index;
                state.hit = 0;
                state.reset_diff = true;
            }
            ReplaceAction::Hit { ticket, index }
                if ticket == state.preview_ticket && state.preview_visible() =>
            {
                state.hit = index
            }
            ReplaceAction::ApplyHit | ReplaceAction::ApplyFile | ReplaceAction::ApplyChecked
                if state.preview_visible() && !state.applied =>
            {
                if state.pending.is_some() || state.task.is_some() {
                    continue;
                }
                let Some(plan) = state
                    .plan
                    .as_ref()
                    .filter(|_| state.current().is_some())
                    .cloned()
                else {
                    state.hint = "请先更新预览再应用替换".into();
                    continue;
                };
                if documents.io.is_some() || documents.pending_load.is_some() {
                    state.hint = "请等待文件任务完成再应用".into();
                    continue;
                }
                let target = match action {
                    ReplaceAction::ApplyHit => Target::Hit(state.file, state.hit),
                    ReplaceAction::ApplyFile => Target::File(state.file),
                    _ => Target::Checked,
                };
                state.request(Work::Apply { plan, target });
                focus.clear();
            }
            ReplaceAction::OpenFile if state.preview_visible() => {
                let path = state
                    .current()
                    .and_then(|plan| plan.search.files.get(state.file))
                    .map(|file| file.path.clone())
                    .or_else(|| {
                        search
                            .current()
                            .and_then(|result| result.files.get(state.file))
                            .map(|file| file.path.clone())
                    });
                if let Some(path) = path {
                    match documents.expand(path.clone(), &mut workspace) {
                        Ok(()) => {
                            if let Some(doc) =
                                documents.documents.iter_mut().find(|doc| doc.path == path)
                            {
                                doc.mode = ViewMode::Edit;
                            }
                            editor.selected_asset = Some(path);
                            state.active = false;
                            focus.clear();
                        }
                        Err(error) => state.hint = error,
                    }
                }
            }
            ReplaceAction::SaveBatch
                if state.preview_visible() && state.pending.is_none() && state.task.is_none() =>
            {
                if let (Some(batch), Some(saving)) = (&state.batch, saving.as_deref_mut()) {
                    let keys = batch.stamps.iter().map(|stamp| stamp.key.clone()).collect();
                    super::super::super::batch_save::request(saving, &mut documents,
                        super::super::super::batch_save::SaveScope::Batch(keys), *content);
                } else {
                    state.hint = "请先应用替换，再保存本批文件的当前草稿".into();
                }
            }
            ReplaceAction::Undo | ReplaceAction::Redo
                if state.preview_visible() && state.pending.is_none() && state.task.is_none() =>
            {
                if let Some(mut batch) = state.batch.take() {
                    state.hint = match tasks::travel(
                        &mut batch,
                        &mut documents,
                        action == ReplaceAction::Redo,
                    ) {
                        Ok(selections) => {
                            state.restores = batch.stamps.iter().cloned().zip(selections).collect();
                            if action == ReplaceAction::Redo {
                                "已整批重做；各文件仍需显式保存"
                            } else {
                                "已整批撤销；恢复每个文件原有内容和历史位置"
                            }
                            .into()
                        }
                        Err(error) => error,
                    };
                    state.batch = Some(batch);
                }
            }
            _ => (),
        }
    }
}

pub(super) fn scroll(
    mut wheels: MessageReader<MouseWheel>,
    editor: Res<MapEditorState>,
    gate: SearchGate,
    state_search: Res<AssetSearchState>,
    mut state: ResMut<AssetReplaceState>,
    mut views: Query<
        (
            &RelativeCursorPosition,
            &ComputedNode,
            &mut ScrollPosition,
            Has<ReplaceFiles>,
        ),
        Or<(With<ReplaceFiles>, With<ReplaceDiff>)>,
    >,
) {
    if state.wheel_owned {
        state.wheel_owned = false;
    }
    let delta: f32 = wheels
        .read()
        .map(|event| {
            -event.y
                * if event.unit == MouseScrollUnit::Line {
                    DIFF_HEIGHT * 3.0
                } else {
                    1.0
                }
        })
        .sum();
    if !state.preview_visible() || gate.blocked(&editor) {
        return;
    }
    let files = state
        .current()
        .map(|plan| plan.search.as_ref())
        .or_else(|| {
            state_search
                .current()
                .filter(|result| Some(result.ticket) == state.search_ticket)
        });
    let file_count = files.map_or(0, |result| result.files.len());
    let diff_count = state
        .current()
        .and_then(|plan| plan.files.iter().find(|file| file.index == state.file))
        .map_or(0, |file| file.after.len());
    for (cursor, node, mut scroll, file_list) in &mut views {
        let height = if file_list { FILE_HEIGHT } else { DIFF_HEIGHT };
        let count = if file_list { file_count } else { diff_count };
        if file_list && state.reset_files || !file_list && state.reset_diff {
            scroll.y = 0.0;
        }
        if delta != 0.0 && cursor.cursor_over() {
            scroll.y = (scroll.y + delta).clamp(
                0.0,
                (count as f32 * height - node.size().y * node.inverse_scale_factor()).max(0.0),
            );
            // 即使已到边界也消费滚轮，中央差异不能把侧栏或地图一起移动。
            state.wheel_owned = true;
        }
    }
    if state.reset_files {
        state.reset_files = false;
    }
    if state.reset_diff {
        state.reset_diff = false;
    }
}
