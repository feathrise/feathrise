//! 中央界面文档复用公共工具栏；保存和历史仍由原文档系统执行。

use super::{components::SharedDocumentTarget, resources::DocumentState, types::SourceKey, ui_layout::UiLayouts};
use crate::{components::SceneEditorActionButton, types::EditorAction};
use crate::ui::vegetation_preset_editor::{PresetWorkspaceTab, VegetationPresetEditorState};
use bevy::prelude::*;
use feathrise_editor_ui::EditorButtonClick;

impl DocumentState {
    pub(crate) fn ui_layout_in_center(&self) -> bool {
        self.in_center() && self.current().is_some_and(|doc| {
            self.expanded.as_ref().is_some_and(|tab| tab.path == doc.path)
                && super::ui_layout::is_document(&doc.path, &doc.source)
        })
    }

    pub(super) fn ui_layout_target(&self) -> Option<SourceKey> {
        self.ui_layout_in_center().then(|| self.current().unwrap().source_key())
    }

    pub(crate) fn ui_layout_command_status(&self, layouts: Option<&UiLayouts>) -> Option<super::types::UiDocumentCommandStatus> {
        if !self.ui_layout_in_center() { return None; }
        let doc = self.current().unwrap();
        Some(super::types::UiDocumentCommandStatus {
            dirty: doc.dirty(),
            undo: doc.history.can_undo(),
            redo: doc.history.can_redo(),
            saving: self.io.is_some() || self.pending_load.is_some(),
            blocked: if !self.visible {
                Some("文档尚未显示")
            } else if self.file_change_pending(&doc.path) || self.is_loading(&doc.path) {
                Some("文件整理或重新载入尚未完成")
            } else if layouts.is_some_and(|layouts| layouts.edit_pending_for(self)) {
                Some("正在采用上一笔界面编辑")
            } else {
                None
            },
        })
    }
}

impl SharedDocumentTarget {
    pub(super) fn matches(&self, documents: &DocumentState) -> bool {
        self.0.is_some() && self.0 == documents.ui_layout_target()
    }
}

/// 在所有按钮接收端之前换绑身份，防止旧按下在另一标签上松开后执行。
pub(super) fn bind(
    mut commands: Commands,
    documents: Res<DocumentState>,
    workspace: Option<Res<VegetationPresetEditorState>>,
    mut buttons: Query<(Entity, &SceneEditorActionButton, &mut EditorButtonClick, Option<&mut SharedDocumentTarget>)>,
) {
    let target = if workspace.as_ref().is_none_or(|workspace| workspace.active_tab == PresetWorkspaceTab::Document) {
        documents.ui_layout_target()
    } else { None };
    for (entity, action, mut click, binding) in &mut buttons {
        if !matches!(action.0, EditorAction::Save | EditorAction::Undo | EditorAction::Redo) { continue; }
        if let Some(mut binding) = binding {
            if binding.0 != target {
                click.invalidate();
                binding.0 = target.clone();
            }
        } else {
            if target.is_some() { click.invalidate(); }
            commands.entity(entity).insert(SharedDocumentTarget(target.clone()));
        }
    }
}
