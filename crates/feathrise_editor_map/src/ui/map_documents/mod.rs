//! F12多地图标签：独立作者草稿与历史，完整三维场景及资源按地图驻留。
//!
//! 【功能设计】切标签不保存、不放弃修改。只有显式关闭才处理未保存责任；
//! 切图先等整笔及保存收口，后台分离共享全局历史，再由原加载门采用目标文档。

mod history;
mod resources;
mod session;
mod systems;
#[cfg(test)]
mod tests;
mod types;
#[cfg(test)]
mod visual_tests;

pub(crate) use resources::{MapDocumentSceneTarget, MapDocuments};
#[cfg(test)]
pub(crate) use resources::MapDocumentSnapshot;
pub(crate) use session::{MapDocumentLoad, MapDocumentTransition, park_document, restore_document};
pub(crate) use systems::{
    finish_map_document_close, handle_map_document_buttons, poll_map_document_history,
    preserve_document_on_external_map_change, restore_map_document_selection, sync_map_documents,
};

use crate::ui::vegetation_preset_editor::PresetWorkspaceTab;
use crate::ui::workspace_tabs::spawn_preset_tab;
use bevy::prelude::*;
use feathrise_map_content::MapId;
#[cfg(feature = "validation")]
pub(crate) use types::MapDocumentButton; // 显式验收只读查找真实地图标签，不生成旧版通用切页动作。
#[cfg(not(feature = "validation"))]
use types::MapDocumentButton;

pub(crate) fn spawn_map_tabs(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    documents: &MapDocuments,
    selected: Option<&MapId>,
    workspace: PresetWorkspaceTab,
) {
    for tab in &documents.tabs {
        spawn_preset_tab(
            parent,
            font,
            &format!("{}{}", tab.name, if tab.dirty { "*" } else { "" }),
            MapDocumentButton::Activate(tab.map_id.clone()),
            MapDocumentButton::Close(tab.map_id.clone()),
            workspace == PresetWorkspaceTab::Map && selected == Some(&tab.map_id),
            false, // 未保存关闭由完整三选一模态处理，不用再次点×直接丢弃。
        );
    }
}
