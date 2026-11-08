//! 诊断位置只发起原文档打开；全文行列换算在后台共享 Arc，主帧仅采用身份一致的选区。

use super::{resources::SourceLocationState, types::*};
use super::super::{components::{DocumentScrollState, DocumentSource}, resources::DocumentState, source, types::ViewMode};
use crate::{
    asset_catalog::{document_format, project_files::EditorProjectFileCatalog},
    resources::{MapEditorSettingsState, MapEditorState},
    standalone::OpenProjectSourceLocation,
    ui::vegetation_preset_editor::VegetationPresetEditorState,
    workbench::WorkbenchModel,
};
use bevy::{input_focus::{FocusCause, InputFocus}, prelude::*,
    tasks::{AsyncComputeTaskPool, futures::check_ready},
    text::{EditableText, FontCx, LayoutCx, TextLayoutInfo}};
use std::{ops::Range, path::{Component, Path}, sync::Arc};

fn blocked(editor: &MapEditorState, model: Option<&WorkbenchModel>, settings: Option<&MapEditorSettingsState>) -> bool {
    !editor.open || editor.close_requested || editor.new_map_dialog_open
        || model.is_some_and(|model| model.modal || model.busy)
        || settings.is_some_and(|settings| settings.open)
}

pub(super) fn current(request: &OpenProjectSourceLocation, catalog: &EditorProjectFileCatalog) -> bool {
    catalog.project_root() == Some(request.project_root.as_path())
        && catalog.source_context() == Some(request.context)
}

fn relative(path: &Path) -> Option<String> {
    let text = path.to_str()?;
    if text.is_empty() || text.len() > 4096 || text.chars().any(char::is_control)
        || !path.components().all(|part| matches!(part, Component::Normal(_))) { return None; }
    let path = text.replace('\\', "/");
    if path.split('/').any(|name| ["target", ".git", ".trash"].iter().any(|hidden| name.eq_ignore_ascii_case(hidden)))
        || document_format(&path).is_none() { return None; }
    Some(path)
}

pub(super) fn receive(
    mut requests: MessageReader<OpenProjectSourceLocation>,
    catalog: Res<EditorProjectFileCatalog>, model: Option<Res<WorkbenchModel>>,
    settings: Option<Res<MapEditorSettingsState>>,
    inputs: Query<(Entity, &EditableText, Option<&DocumentSource>)>, mut documents: ResMut<DocumentState>,
    mut workspace: ResMut<VegetationPresetEditorState>, mut editor: ResMut<MapEditorState>,
    mut state: ResMut<SourceLocationState>, mut focus: ResMut<InputFocus>,
) {
    let Some(request) = requests.read().last().cloned() else { return; };
    state.ticket = state.ticket.wrapping_add(1);
    state.pending = None;
    state.ready = None;
    state.reveal = None;
    // 这是已经由浮层确认的显式意图；它自身的关闭释放帧仍拥有公共鼠标门，不能因此丢弃定位。
    if blocked(&editor, model.as_deref(), settings.as_deref()) || !current(&request, &catalog) { return; }
    // 当前源码和真实焦点拥有未提交输入；其他隐藏表单的回绑定队列并非作者输入事务，
    // 不能因选中对象时恰好重绑名称等字段而吞掉唯一的诊断打开意图。
    if inputs.iter().any(|(entity, input, source)| {
        (focus.get() == Some(entity) || source.is_some_and(|source| source.key.is_some()))
            && (input.is_composing() || input.pending_paste.is_some() || !input.pending_edits.is_empty())
    }) {
        editor.status = "请先确认中文输入或等待粘贴完成，再定位编译诊断；草稿已保留".into();
        return;
    }
    let Some(relative) = relative(&request.path).filter(|_| request.line > 0 && request.column > 0) else {
        editor.status = "编译诊断没有有效的工程文件行列位置".into(); return;
    };
    // 完整独立编辑器没有 WorkbenchModel 时，与工程浏览器一样使用已绑定路径及 context=0。
    let bound = feathrise_editor_core::editor_project_paths();
    let assets = if let Some(model) = model.as_ref() { model.asset_root.as_deref() }
        else { bound.map(|paths| paths.asset_root()) };
    let opened = documents.bind_project_source(&request.project_root, assets)
        .and_then(|_| source::project_key(documents.asset_root(), &request.project_root, &relative))
        .and_then(|path| { documents.expand(path.clone(), &mut workspace)?; Ok(path) });
    match opened {
        Ok(path) => {
            focus.clear(); // 隐藏的前一文件不能在等待原加载期间继续接收新输入。
            state.pending = Some(PendingLocation { request, path, document: None });
            editor.status = "正在原中央文档中定位编译诊断…".into();
        }
        Err(error) => editor.status = error,
    }
}

pub(super) fn prepare(
    editor: Res<MapEditorState>, catalog: Res<EditorProjectFileCatalog>, model: Option<Res<WorkbenchModel>>,
    settings: Option<Res<MapEditorSettingsState>>,
    host: Option<Res<feathrise_editor_core::EditorHostOverlay>>,
    mut documents: ResMut<DocumentState>, mut state: ResMut<SourceLocationState>,
) {
    if let Some(result) = state.task.as_mut().and_then(check_ready) {
        state.task = None;
        if result.ticket == state.ticket { state.ready = Some(result); }
    }
    let Some(pending) = state.pending.as_ref() else { return; };
    if blocked(&editor, model.as_deref(), settings.as_deref()) || !current(&pending.request, &catalog)
        || documents.expanded.as_ref().is_none_or(|tab| tab.path != pending.path) {
        state.pending = None; state.ready = None; return;
    }
    if host.is_some_and(|host| host.active) { return; } // 诊断窗关闭释放帧只暂存意图，等原宿主门真正释放。
    if documents.is_loading(&pending.path) { return; } // 等本次载入采用后再固定代次，不能用旧正文行列任务吞掉诊断意图。
    let Some(doc) = documents.current_mut().filter(|doc| doc.path == pending.path) else {
        if !documents.error.is_empty() { state.pending = None; state.ready = None; }
        return;
    };
    let key = doc.source_key();
    let revision = doc.revision;
    if pending.document.as_ref().is_some_and(|(expected, before)| *expected != key || *before != revision) {
        state.pending = None; state.ready = None; return; // 作者已改字，不能用迟到行列抢回新光标。
    }
    doc.mode = ViewMode::Edit;
    if state.task.is_some() || state.ready.is_some() { return; }
    let line = pending.request.line;
    let column = pending.request.column;
    let text = Arc::clone(&doc.source);
    let ticket = state.ticket;
    state.pending.as_mut().unwrap().document = Some((key.clone(), revision));
    state.task = Some(AsyncComputeTaskPool::get().spawn(async move {
        let (selection, clamped) = position(&text, line, column);
        PositionResult { ticket, document: key, revision, selection, clamped }
    }));
}

/// 已保存诊断面对更短草稿时夹到真实末尾，并由回显说明；换算不写正文、历史或磁盘。
pub(super) fn position(text: &str, line: u32, column: u32) -> (Range<usize>, bool) {
    let mut start = 0;
    let mut selected = "";
    let mut clamped = true;
    for (index, value) in text.split('\n').enumerate() {
        selected = value;
        if index + 1 >= line as usize { clamped = false; break; }
        start += value.len() + 1;
    }
    if clamped { start = text.len().saturating_sub(selected.len()); }
    let mut chars = selected.char_indices().map(|(at, character)| (at, character.len_utf8()))
        .chain(std::iter::once((selected.len(), 0)));
    let (at, length) = chars.nth(column.saturating_sub(1) as usize).unwrap_or_else(|| {
        clamped = true; (selected.len(), 0)
    });
    (start + at..start + at + length, clamped)
}

pub(super) fn select(
    editor: Res<MapEditorState>, model: Option<Res<WorkbenchModel>>,
    settings: Option<Res<MapEditorSettingsState>>,
    host: Option<Res<feathrise_editor_core::EditorHostOverlay>>,
    mut documents: ResMut<DocumentState>, mut state: ResMut<SourceLocationState>,
    mut sources: Query<(Entity, &DocumentSource, &mut EditableText)>,
    mut font: ResMut<FontCx>, mut layout: ResMut<LayoutCx>, mut focus: ResMut<InputFocus>,
) {
    if state.ready.is_none() { return; }
    if host.is_some_and(|host| host.active) { return; }
    if blocked(&editor, model.as_deref(), settings.as_deref()) || focus.get().is_some() {
        state.pending = None; state.ready = None; return;
    }
    let result = state.ready.as_ref().unwrap();
    let Some(doc) = documents.current().filter(|doc| documents.visible && doc.mode == ViewMode::Edit
        && doc.source_key() == result.document && doc.revision == result.revision) else { return; };
    let Ok((entity, binding, mut text)) = sources.single_mut() else { return; };
    if binding.key.as_ref() != Some(&result.document) { return; }
    if text.is_composing() || text.pending_paste.is_some() { state.pending = None; state.ready = None; return; }
    if !text.pending_edits.is_empty() { return; } // 原文件绑定的 TextStart 必须先由官方输入队列完成。
    let selection = result.selection.clone();
    let clamped = result.clamped;
    text.editor.driver(&mut font.context, &mut layout.0).select_byte_range(selection.start, selection.end);
    focus.set(entity, FocusCause::Navigated);
    state.reveal = Some(Reveal { source: entity, document: doc.source_key(), revision: doc.revision, selection });
    let doc = documents.current_mut().unwrap();
    doc.status = if doc.dirty() || clamped {
        "已定位编译时保存版本的行列；当前未保存草稿已保留，位置可能已变化"
    } else { "已定位编译诊断 · 使用原中央源码与文档历史" }.into();
    state.pending = None;
    state.ready = None;
}

pub(super) fn reveal(
    documents: Res<DocumentState>, mut state: ResMut<SourceLocationState>,
    mut sources: Query<(Entity, &DocumentSource, &mut EditableText, &ComputedNode, &TextLayoutInfo, &mut DocumentScrollState)>,
) {
    let Some(target) = state.reveal.take() else { return; };
    let Some(doc) = documents.current().filter(|doc| documents.visible && doc.source_key() == target.document
        && doc.revision == target.revision) else { return; };
    let Ok((entity, binding, mut text, node, layout, mut preservation)) = sources.single_mut() else { return; };
    if entity == target.source && binding.key.as_ref() == Some(&doc.source_key())
        && !text.is_composing() && text.pending_edits.is_empty()
        && text.editor.raw_selection().text_range() == target.selection {
        super::super::navigation::reveal_selection(node, layout, &mut text.viewport, &mut preservation);
    }
}
