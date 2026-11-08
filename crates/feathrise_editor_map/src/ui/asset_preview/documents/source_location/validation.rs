//! 仅显式 validation 的原文档只读探针；调用方先核验本轮自有夹具，不向文档写入成功标记。

use feathrise_editor_ui::InteractionButton;
use super::super::{components::{DocumentAction, DocumentSource}, history, resources::DocumentState, types::{Document, ViewMode}};
use bevy::{input_focus::InputFocus, prelude::*, text::EditableText};
use feathrise_editor_documents_model::history::components::HistoryAction;
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug)]
pub enum SourceLocationAction { Edit, Save, Undo, Redo, Map }

/// 即使尚未打开文档也报告真实等待门，防止验收只拿到 null 后一直空等。
pub fn source_location_navigation_probe(world: &mut World) -> Value {
    let mut inputs = world.query::<(Entity, &EditableText, Option<&DocumentSource>, Option<&Name>)>();
    let focus = world.get_resource::<InputFocus>().and_then(InputFocus::get);
    let busy: Vec<_> = inputs.iter(world).filter(|(_, input, _, _)| input.is_composing()
        || input.pending_paste.is_some() || !input.pending_edits.is_empty()).take(16)
        .map(|(entity, input, source, name)| json!({"entity":entity.to_bits(),"focused":Some(entity) == focus,
            "bound_source":source.and_then(|source| source.key.as_ref()).map(|key| key.path.as_str()),
            "name":name.map(Name::as_str),"pending":input.pending_edits.len(),"composing":input.is_composing(),"paste":input.pending_paste.is_some()})).collect();
    let documents = world.get_resource::<DocumentState>();
    let state = world.get_resource::<super::resources::SourceLocationState>();
    let catalog = world.get_resource::<crate::asset_catalog::project_files::EditorProjectFileCatalog>();
    let editor = world.get_resource::<crate::resources::MapEditorState>();
    json!({"focus":focus.map(Entity::to_bits),"busy_inputs":busy,
        "editor":editor.map(|editor| json!({"open":editor.open,"close_requested":editor.close_requested,"new_map":editor.new_map_dialog_open,"status":editor.status})),
        "catalog_root":catalog.and_then(|catalog| catalog.project_root()),"catalog_context":catalog.and_then(|catalog| catalog.source_context()),
        "bound_root":feathrise_editor_core::editor_project_paths().map(|paths| paths.project_root()),
        "host_overlay":world.get_resource::<feathrise_editor_core::EditorHostOverlay>().is_some_and(|host| host.active),
        "documents":documents.map(|documents| json!({"root":documents.project_root,"active":documents.active,
            "expanded":documents.expanded.as_ref().map(|tab| &tab.path),"error":documents.error,"io":documents.io.is_some(),
            "pending_load":documents.pending_load.as_ref().map(|request| &request.path),"visible":documents.visible,"central_active":documents.central_active})),
        "location":state.map(|state| json!({"ticket":state.ticket,"pending":state.pending.as_ref().map(|pending| &pending.path),
            "task":state.task.is_some(),"ready":state.ready.is_some()}))})
}

fn document_path(doc: &Document, documents: &DocumentState) -> String {
    doc.disk.path.strip_prefix(documents.project_root.as_deref().unwrap_or(documents.asset_root()))
        .unwrap_or(&doc.disk.path).to_string_lossy().replace('\\', "/")
}

fn document_snapshot(doc: &Document, documents: &DocumentState, source_limit: usize) -> Value {
    let (undo_len, redo_len) = history::validation_lengths(&doc.history);
    json!({"path":document_path(doc, documents), "document_key":doc.path, "disk_path":doc.disk.path,
        "source":(doc.source.len() <= source_limit).then(|| doc.source.to_string()), "source_bytes":doc.source.len(),
        "disk_digest":doc.disk.digest, "generation":doc.generation, "revision":doc.revision,
        "content_revision":doc.content_revision(), "saved_revision":doc.saved_revision, "dirty":doc.dirty(),
        "can_undo":doc.history.can_undo(), "can_redo":doc.history.can_redo(), "undo_len":undo_len, "redo_len":redo_len})
}

/// 只读取生产 DocumentState、唯一原生输入、实际焦点/选区/滚动，不读盘或修改作者状态。
pub fn source_location_probe(world: &mut World) -> Option<Value> {
    let mut inputs = world.query::<(Entity, &DocumentSource, &EditableText)>();
    let documents = world.get_resource::<DocumentState>()?;
    let doc = documents.current()?;
    let mut result = document_snapshot(doc, documents, 64 * 1024);
    result["project_root"] = json!(documents.project_root);
    result["visible"] = json!(documents.visible);
    result["central_active"] = json!(documents.central_active);
    result["edit_mode"] = json!(doc.mode == ViewMode::Edit);
    result["status"] = json!(doc.status);
    result["documents"] = Value::Array(documents.documents.iter()
        .map(|doc| document_snapshot(doc, documents, 8 * 1024)).collect());
    for field in ["input_entity", "selection", "selected_text", "line", "column", "focused", "native_matches", "native_text", "pending_edits", "composing", "scroll"] {
        result[field] = Value::Null;
    }
    let key = doc.source_key();
    if let Some((entity, _, input)) = inputs.iter(world).find(|(_, binding, _)| binding.key.as_ref() == Some(&key)) {
        let selection = input.editor.raw_selection().text_range();
        let start = selection.start.min(doc.source.len());
        let prefix = doc.source.get(..start)?;
        let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
        let column = prefix.rsplit('\n').next().unwrap_or_default().chars().count() + 1;
        result["input_entity"] = json!(entity.to_bits());
        result["selection"] = json!([selection.start, selection.end]);
        result["selected_text"] = json!(doc.source.get(selection).filter(|text| text.len() <= 4096));
        result["line"] = json!(line);
        result["column"] = json!(column);
        result["focused"] = json!(world.get_resource::<InputFocus>().is_some_and(|focus| focus.get() == Some(entity)));
        result["native_matches"] = json!(input.value() == doc.source.as_ref());
        result["native_text"] = json!((doc.source.len() <= 64 * 1024).then(|| input.value().to_string()));
        result["pending_edits"] = json!(input.pending_edits.len());
        result["composing"] = json!(input.is_composing());
        result["scroll"] = json!([input.viewport.offset.x, input.viewport.offset.y]);
    }
    Some(result)
}

/// 返回原唯一输入而不排队编辑；验收驱动通过官方 TextEdit 队列产生真实捕获和文档历史。
pub fn source_location_input_entity(world: &mut World, expected_path: &str) -> Option<Entity> {
    let mut inputs = world.query::<(Entity, &DocumentSource, &EditableText)>();
    let documents = world.get_resource::<DocumentState>()?;
    let doc = documents.current()?;
    if !documents.visible || doc.mode != ViewMode::Edit || document_path(doc, documents) != expected_path { return None; }
    let key = doc.source_key();
    inputs.iter(world).find_map(|(entity, binding, input)| {
        (binding.key.as_ref() == Some(&key) && !input.is_composing()
            && input.pending_paste.is_none() && input.pending_edits.is_empty()
            && input.value() == doc.source.as_ref()).then_some(entity)
    })
}

/// 只查找原工具按钮；保存、撤销和返回地图仍由原交互系统执行，不能把查找到实体当作已完成。
pub fn source_location_action_entity(world: &mut World, expected_path: &str, action: SourceLocationAction) -> Option<Entity> {
    use crate::ui::map_documents::{MapDocumentButton, MapDocuments};
    let mut buttons = world.query_filtered::<(Entity, Option<&DocumentAction>, Option<&HistoryAction>, Option<&MapDocumentButton>,
        &Node, &ComputedNode, &InheritedVisibility, Has<bevy::ui::InteractionDisabled>), With<InteractionButton>>();
    let documents = world.get_resource::<DocumentState>()?;
    let doc = documents.current()?;
    if !documents.visible || document_path(doc, documents) != expected_path
        || world.get_resource::<feathrise_editor_core::EditorHostOverlay>().is_some_and(|host| host.active) { return None; }
    // 中央真实地图标签携带稳定地图ID；只返回当前仍打开的同图标题，不误点关闭键或另一个地图。
    let current_map = if matches!(action, SourceLocationAction::Map) {
        let map = world.get_resource::<crate::resources::MapEditorState>()?.selected_map_id.as_ref()?;
        if !world.get_resource::<MapDocuments>()?.contains(map) { return None; }
        Some(map)
    } else { None };
    buttons.iter(world).find_map(|(entity, document, history, map, node, computed, visible, disabled)| {
        let matches = match action {
            SourceLocationAction::Edit => document == Some(&DocumentAction::Edit),
            SourceLocationAction::Save => document == Some(&DocumentAction::Save),
            SourceLocationAction::Undo => history == Some(&HistoryAction::Undo),
            SourceLocationAction::Redo => history == Some(&HistoryAction::Redo),
            SourceLocationAction::Map => map.is_some_and(|button| matches!(button,
                MapDocumentButton::Activate(id) if Some(id) == current_map)),
        };
        (matches && !disabled && node.display != Display::None && !computed.is_empty() && visible.get()).then_some(entity)
    })
}

#[cfg(test)]
#[path = "validation_tests.rs"]
mod tests;
