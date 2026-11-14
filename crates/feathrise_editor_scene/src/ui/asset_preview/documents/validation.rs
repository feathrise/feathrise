//! 自有验收工程的普通文本会话探针；读写与历史均复用生产文档工具。

use super::{history::EditContext, resources::DocumentState};
use crate::ui::vegetation_preset_editor::VegetationPresetEditorState;
use bevy::prelude::*;
use serde_json::{Value, json};

pub(crate) const PATH: &str = "feathrise-close-validation.txt";

pub(crate) fn open(world: &mut World) -> Result<(), String> {
    world.resource_scope(|world, mut state: Mut<DocumentState>| {
        let mut workspace = world.resource_mut::<VegetationPresetEditorState>();
        // 原展开入口预留会话预算；下一次生产sync_selection/drive_tasks读取磁盘。
        state.expand(PATH.into(), &mut workspace)
    })
}

pub(crate) fn ready(world: &World) -> Result<bool, String> {
    let state = world
        .get_resource::<DocumentState>()
        .ok_or("普通文档工具未安装")?;
    if state.io.is_some() || state.pending_load.is_some() {
        return Ok(false);
    }
    if !state.error.is_empty() {
        return Err(state.error.clone());
    }
    Ok(state.documents.iter().any(|doc| doc.path == PATH))
}

/// 制造真实的两步编辑并撤销一步；关闭取消和失败后必须保留这份可撤销/可重做草稿。
pub(crate) fn edit(world: &mut World) -> Result<Value, String> {
    let mut state = world.resource_mut::<DocumentState>();
    let doc = state
        .documents
        .iter_mut()
        .find(|doc| doc.path == PATH)
        .ok_or("关闭验收普通文档尚未读取")?;
    if doc.dirty() || doc.source.len() > 4096 {
        return Err("关闭验收要求自有、已保存且有界的文本基线".into());
    }
    let original = doc.source.clone();
    doc.edit_with(
        &format!("{original}关闭前未保存的修改\n"),
        EditContext::default(),
    );
    doc.edit_with(
        &format!("{original}第二步临时修改\n"),
        EditContext::default(),
    );
    doc.travel(false).ok_or("普通文本撤销没有实际历史")?;
    drop(state);
    snapshot(world).ok_or_else(|| "普通文档探针丢失".into())
}

pub(crate) fn snapshot(world: &World) -> Option<Value> {
    let state = world.get_resource::<DocumentState>()?;
    let doc = state.documents.iter().find(|doc| doc.path == PATH)?;
    if doc.source.len() > 4096 {
        return None;
    }
    Some(json!({
        "path": doc.path, "generation": doc.generation,
        "source": &*doc.source, "disk_digest": doc.disk.digest,
        "revision": doc.revision, "content_revision": doc.content_revision(),
        "saved_revision": doc.saved_revision, "dirty": doc.dirty(),
        "can_undo": doc.history.can_undo(), "can_redo": doc.history.can_redo()
    }))
}

/// 仅回读自有文件桌面夹具已经通过原加载器采用的正文，不直接安装文档或预期成功状态。
#[cfg(feature = "validation")]
pub(crate) fn loaded_text(world: &World, path: &str) -> Option<(String, bool)> {
    let state = world.get_resource::<DocumentState>()?;
    let doc = state.documents.iter().find(|doc| doc.path == path && doc.source.len() <= 4096)?;
    Some((doc.source.to_string(), doc.dirty()))
}

#[cfg(feature = "validation")]
pub(crate) fn redo(world: &mut World) -> Result<(), String> {
    let mut state = world.resource_mut::<DocumentState>();
    let document = state.documents.iter_mut().find(|doc| doc.path == PATH)
        .ok_or("语言验收的文档草稿已消失")?;
    document.travel(true).ok_or("切换语言后无法重做普通文本编辑")?;
    Ok(())
}
