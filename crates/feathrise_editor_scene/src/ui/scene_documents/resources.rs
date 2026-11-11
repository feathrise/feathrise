//! 多场景作者文档所有权；完整三维运行时由场景域驻留，文档保存各自作者源和历史。

use super::history::SceneHistoryOrder;
use super::types::{SceneDocumentTab, SceneDocumentUi};
use crate::authoring::{EditorAuthoringSession, EditorCommand, SparseTerrainAuthoringState};
use crate::resources::EditorViewportSelectionState;
use bevy::{prelude::*, tasks::Task};
use feathrise_scene_content::{
    SceneAuthorPreviewHandles, SceneBoundariesAsset, SceneId, ScenePlacementsAsset, ScenePortalsAsset,
    SparseHeightLiveOverride,
};
use std::collections::BTreeMap;

pub(crate) struct SceneDocumentSnapshot {
    pub(crate) session: EditorAuthoringSession,
    pub(crate) sparse: SparseTerrainAuthoringState,
    pub(crate) heights: SparseHeightLiveOverride,
    pub(crate) selection: EditorViewportSelectionState,
    pub(crate) ui: SceneDocumentUi,
    pub(crate) dirty: bool,
    pub(super) history_order: SceneHistoryOrder,
}

#[cfg(test)]
impl SceneDocumentSnapshot {
    /// 水体全局引用回归复用真实停放槽，不为测试暴露私有历史排序细节。
    pub(crate) fn with_test_session(session: EditorAuthoringSession) -> Self {
        Self {
            session,
            sparse: Default::default(), heights: Default::default(), selection: Default::default(),
            ui: SceneDocumentUi::capture(&crate::resources::SceneEditorState::default(),
                &crate::ui::gltf_browser::EditorGltfBrowserState::default()),
            dirty: true, history_order: Default::default(),
        }
    }
}

pub(crate) struct SceneHistoryTransfer {
    pub(crate) source: Option<SceneId>,
    pub(crate) target: SceneId,
    pub(crate) source_undo: Vec<EditorCommand>,
    pub(crate) source_redo: Vec<EditorCommand>,
    pub(crate) target_undo: Vec<EditorCommand>,
    pub(crate) target_redo: Vec<EditorCommand>,
    pub(crate) target_document: Option<SceneDocumentSnapshot>,
    pub(crate) preview: Option<SceneDocumentPreview>,
    pub(super) source_order: SceneHistoryOrder,
    pub(super) source_dirty: bool,
}

pub(crate) struct SceneDocumentPreview {
    pub(crate) placements: ScenePlacementsAsset,
    pub(crate) portals: ScenePortalsAsset,
    pub(crate) boundaries: Option<SceneBoundariesAsset>,
}

#[derive(Clone)]
pub(crate) struct SceneDocumentSceneTarget {
    pub(crate) scene_id: SceneId,
    pub(crate) scene: feathrise_scene_core::state::ScenePhase,
    pub(crate) name: String,
    pub(crate) spawn: Vec3,
}

/// 全部大对象只移动所有权；一份历史交接任务限制切换场景期间的CPU及快照并发。
#[derive(Resource, Default)]
pub(crate) struct SceneDocuments {
    pub(crate) tabs: Vec<SceneDocumentTab>,
    pub(crate) parked: BTreeMap<SceneId, SceneDocumentSnapshot>,
    pub(crate) history_task: Option<Task<SceneHistoryTransfer>>,
    pub(crate) cleanup_history: Option<Task<(Vec<EditorCommand>, Vec<EditorCommand>)>>,
    pub(crate) pending_history: Option<SceneHistoryTransfer>,
    pub(crate) preparing_target: Option<SceneDocumentSceneTarget>,
    pub(crate) preview_storage: Option<SceneAuthorPreviewHandles>, // 在Assets移出大快照前保持强引用，避免主线程自动析构。
    pub(crate) parked_preview_storage: BTreeMap<SceneId, SceneAuthorPreviewHandles>, // 后台场景继续持有原预览资产，切回不换句柄。
    pub(crate) retiring_preview_storage: BTreeMap<SceneId, SceneAuthorPreviewHandles>, // 实体回收完毕才从Assets移出大预览源。
    pub(crate) pending_selection: Option<(SceneId, SceneDocumentUi)>,
    pub(crate) close_requested: Option<SceneId>,
    pub(crate) closing: Option<SceneId>,
    pub(crate) close_ready: bool,
    pub(crate) active_metadata: Option<(SceneId, String, bool)>, // 鼠标游标逐帧变化不触发标签线性查找。
}

impl SceneDocuments {
    /// 原生窗口低频退出检查使用原标签/停放快照脏位，不遍历场景实体或历史正文。
    pub(crate) fn exit_blocker(&self, active: Option<&SceneId>, unedited_active_migration: bool) -> Option<&'static str> {
        if self.history_task.is_some() || self.pending_history.is_some() || self.preparing_target.is_some()
            || self.closing.is_some() || self.close_requested.is_some() {
            return Some("场景文档切换、关闭或历史交接尚未完成");
        }
        let unedited_parked = |map: &SceneId| self.parked.get(map)
            .is_some_and(|document| document.session.is_unedited_load_migration_for(map));
        (self.tabs.iter().any(|tab| tab.dirty && !(unedited_active_migration && active == Some(&tab.scene_id)) && !unedited_parked(&tab.scene_id))
            || self.parked.iter().any(|(map, document)| !unedited_parked(map) && (document.dirty || document.session.has_scene_data_changes())))
            .then_some("存在未保存的场景文档，请先逐张保存或使用原场景文档关闭确认丢弃")
    }

    pub(crate) fn contains(&self, map: &SceneId) -> bool {
        self.tabs.iter().any(|tab| &tab.scene_id == map)
    }

    pub(crate) fn upsert(&mut self, map: SceneId, name: String, dirty: bool) -> bool {
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.scene_id == map) {
            if tab.name == name && tab.dirty == dirty {
                return false;
            }
            tab.name = name;
            tab.dirty = dirty;
        } else {
            self.tabs.push(SceneDocumentTab {
                scene_id: map,
                name,
                dirty,
            });
        }
        true
    }
}
