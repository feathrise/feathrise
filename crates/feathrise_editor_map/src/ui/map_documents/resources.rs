//! 多地图作者文档所有权；完整三维运行时由场景域驻留，文档保存各自作者源和历史。

use super::history::MapHistoryOrder;
use super::types::{MapDocumentTab, MapDocumentUi};
use crate::authoring::{EditorAuthoringSession, EditorCommand, SparseTerrainAuthoringState};
use crate::resources::EditorViewportSelectionState;
use bevy::{prelude::*, tasks::Task};
use feathrise_map_content::{
    MapAuthorPreviewHandles, MapBoundariesAsset, MapId, MapPlacementsAsset, MapPortalsAsset,
    SparseHeightLiveOverride,
};
use std::collections::BTreeMap;

pub(crate) struct MapDocumentSnapshot {
    pub(crate) session: EditorAuthoringSession,
    pub(crate) sparse: SparseTerrainAuthoringState,
    pub(crate) heights: SparseHeightLiveOverride,
    pub(crate) selection: EditorViewportSelectionState,
    pub(crate) ui: MapDocumentUi,
    pub(crate) dirty: bool,
    pub(super) history_order: MapHistoryOrder,
}

#[cfg(test)]
impl MapDocumentSnapshot {
    /// 水体全局引用回归复用真实停放槽，不为测试暴露私有历史排序细节。
    pub(crate) fn with_test_session(session: EditorAuthoringSession) -> Self {
        Self {
            session,
            sparse: Default::default(), heights: Default::default(), selection: Default::default(),
            ui: MapDocumentUi::capture(&crate::resources::MapEditorState::default(),
                &crate::ui::gltf_browser::EditorGltfBrowserState::default()),
            dirty: true, history_order: Default::default(),
        }
    }
}

pub(crate) struct MapHistoryTransfer {
    pub(crate) source: Option<MapId>,
    pub(crate) target: MapId,
    pub(crate) source_undo: Vec<EditorCommand>,
    pub(crate) source_redo: Vec<EditorCommand>,
    pub(crate) target_undo: Vec<EditorCommand>,
    pub(crate) target_redo: Vec<EditorCommand>,
    pub(crate) target_document: Option<MapDocumentSnapshot>,
    pub(crate) preview: Option<MapDocumentPreview>,
    pub(super) source_order: MapHistoryOrder,
    pub(super) source_dirty: bool,
}

pub(crate) struct MapDocumentPreview {
    pub(crate) placements: MapPlacementsAsset,
    pub(crate) portals: MapPortalsAsset,
    pub(crate) boundaries: Option<MapBoundariesAsset>,
}

#[derive(Clone)]
pub(crate) struct MapDocumentSceneTarget {
    pub(crate) map_id: MapId,
    pub(crate) scene: feathrise_scene_core::state::ScenePhase,
    pub(crate) name: String,
    pub(crate) spawn: Vec3,
}

/// 全部大对象只移动所有权；一份历史交接任务限制切图期间的CPU及快照并发。
#[derive(Resource, Default)]
pub(crate) struct MapDocuments {
    pub(crate) tabs: Vec<MapDocumentTab>,
    pub(crate) parked: BTreeMap<MapId, MapDocumentSnapshot>,
    pub(crate) history_task: Option<Task<MapHistoryTransfer>>,
    pub(crate) cleanup_history: Option<Task<(Vec<EditorCommand>, Vec<EditorCommand>)>>,
    pub(crate) pending_history: Option<MapHistoryTransfer>,
    pub(crate) preparing_target: Option<MapDocumentSceneTarget>,
    pub(crate) preview_storage: Option<MapAuthorPreviewHandles>, // 在Assets移出大快照前保持强引用，避免主线程自动析构。
    pub(crate) parked_preview_storage: BTreeMap<MapId, MapAuthorPreviewHandles>, // 后台场景继续持有原预览资产，切回不换句柄。
    pub(crate) retiring_preview_storage: BTreeMap<MapId, MapAuthorPreviewHandles>, // 实体回收完毕才从Assets移出大预览源。
    pub(crate) pending_selection: Option<(MapId, MapDocumentUi)>,
    pub(crate) close_requested: Option<MapId>,
    pub(crate) closing: Option<MapId>,
    pub(crate) close_ready: bool,
    pub(crate) active_metadata: Option<(MapId, String, bool)>, // 鼠标游标逐帧变化不触发标签线性查找。
}

impl MapDocuments {
    /// 原生窗口低频退出检查使用原标签/停放快照脏位，不遍历地图实体或历史正文。
    pub(crate) fn exit_blocker(&self, active: Option<&MapId>, unedited_active_migration: bool) -> Option<&'static str> {
        if self.history_task.is_some() || self.pending_history.is_some() || self.preparing_target.is_some()
            || self.closing.is_some() || self.close_requested.is_some() {
            return Some("地图文档切换、关闭或历史交接尚未完成");
        }
        let unedited_parked = |map: &MapId| self.parked.get(map)
            .is_some_and(|document| document.session.is_unedited_load_migration_for(map));
        (self.tabs.iter().any(|tab| tab.dirty && !(unedited_active_migration && active == Some(&tab.map_id)) && !unedited_parked(&tab.map_id))
            || self.parked.iter().any(|(map, document)| !unedited_parked(map) && (document.dirty || document.session.has_map_data_changes())))
            .then_some("存在未保存的地图文档，请先逐张保存或使用原地图文档关闭确认丢弃")
    }

    pub(crate) fn contains(&self, map: &MapId) -> bool {
        self.tabs.iter().any(|tab| &tab.map_id == map)
    }

    pub(crate) fn upsert(&mut self, map: MapId, name: String, dirty: bool) -> bool {
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.map_id == map) {
            if tab.name == name && tab.dirty == dirty {
                return false;
            }
            tab.name = name;
            tab.dirty = dirty;
        } else {
            self.tabs.push(MapDocumentTab {
                map_id: map,
                name,
                dirty,
            });
        }
        true
    }
}
