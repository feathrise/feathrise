//! 工程关闭调用原文件“保存全部”事务，覆盖全部会话而非仅可见标签。

use super::{batch_save, resources::DocumentState};
use crate::standalone::project_close::types::{ProjectCloseSnapshot, SaveItem, SaveTarget};
use bevy::prelude::*;

pub(crate) fn collect(
    world: &World,
    snapshot: &mut ProjectCloseSnapshot,
    items: &mut Vec<SaveItem>,
) {
    let Some(state) = world.get_resource::<DocumentState>() else {
        return;
    };
    if state.io.is_some() || state.pending_load.is_some() || state.json.exit_pending() {
        snapshot.pending("文件载入、保存或JSON格式化尚未完成");
    }
    let mut dirty = false;
    for document in &state.documents {
        if document.dirty() {
            snapshot.unsaved.push(format!("文件：{}", super::source::display_path(&document.path)));
            dirty = true;
        }
    }
    if dirty {
        items.push(SaveItem {
            target: SaveTarget::Documents,
            label: "文件文档（含隐藏草稿）".into(),
        });
    }
}

pub(crate) fn request(world: &mut World) -> Result<(), String> {
    let content = *world.get_resource::<feathrise_editor_documents_model::DocumentContentOperations>()
        .ok_or("文档内容处理尚未初始化，草稿保持")?;
    world.resource_scope(|world, mut documents: Mut<DocumentState>| {
        let mut report = world
            .get_resource_mut::<batch_save::DocumentBatchSaveState>()
            .ok_or("原文档批量保存事务尚未初始化")?;
        batch_save::request(&mut report, &mut documents, batch_save::SaveScope::All, content);
        if documents.saved_play_pending() {
            Ok(())
        } else {
            report.project_close_result().map(|_| ())
        }
    })
}

pub(crate) fn poll(world: &World) -> Result<bool, String> {
    let documents = world
        .get_resource::<DocumentState>()
        .ok_or("文件会话已变化")?;
    if documents.io.is_some() || documents.saved_play_pending() {
        return Ok(false);
    }
    world
        .get_resource::<batch_save::DocumentBatchSaveState>()
        .ok_or("文件保存回执已丢失")?
        .project_close_result()?;
    if let Some(document) = documents.documents.iter().find(|document| document.dirty()) {
        return Err(format!(
            "文件{}未完成保存：{}",
            super::source::display_path(&document.path), document.status
        ));
    }
    Ok(true)
}
