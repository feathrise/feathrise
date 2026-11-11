//! F12多场景标签：独立作者草稿与历史，完整三维场景及资源按场景驻留。
//!
//! 【功能设计】切标签不保存、不放弃修改。只有显式关闭才处理未保存责任；
//! 切换场景先等整笔及保存收口，后台分离共享全局历史，再由原加载门采用目标文档。

mod history;
mod resources;
mod session;
mod systems;
#[cfg(test)]
mod tests;
mod types;
#[cfg(test)]
mod visual_tests;

pub(crate) use resources::{SceneDocumentSceneTarget, SceneDocuments};
#[cfg(test)]
pub(crate) use resources::SceneDocumentSnapshot;
pub(crate) use session::{SceneDocumentLoad, SceneDocumentTransition, park_document, restore_document};
pub(crate) use systems::{
    finish_scene_document_close, handle_scene_document_buttons, poll_scene_document_history,
    preserve_document_on_external_scene_change, restore_scene_document_selection, sync_scene_documents,
};

use crate::ui::vegetation_preset_editor::PresetWorkspaceTab;
use crate::ui::workspace_tabs::spawn_preset_tab;
use bevy::prelude::*;
use feathrise_scene_content::SceneId;
#[cfg(feature = "validation")]
pub(crate) use types::SceneDocumentButton; // 显式验收只读查找真实场景标签，不生成旧版通用切页动作。
#[cfg(not(feature = "validation"))]
use types::SceneDocumentButton;

pub(crate) fn spawn_scene_tabs(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    documents: &SceneDocuments,
    selected: Option<&SceneId>,
    workspace: PresetWorkspaceTab,
) {
    for tab in &documents.tabs {
        spawn_preset_tab(
            parent,
            font,
            &format!("{}{}", tab.name, if tab.dirty { "*" } else { "" }),
            SceneDocumentButton::Activate(tab.scene_id.clone()),
            SceneDocumentButton::Close(tab.scene_id.clone()),
            workspace == PresetWorkspaceTab::Scene && selected == Some(&tab.scene_id),
            false, // 未保存关闭由完整三选一模态处理，不用再次点×直接丢弃。
        );
    }
}
