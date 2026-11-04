//! 地图页的小型表现快照和单个所选对象的有界查名游标，不复制整棵目录树。

use crate::types::{MapBrowserEntry, MapBrowserIndexSignature, MapBrowserItem};
use feathrise_map_content::MapId;
use std::sync::Arc;

/// 所选目录地图的固定元数据，不复刻网格或环境作者表单。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum MapPropertiesField {
    #[default]
    Name,
    Id,
    Kind,
    Notes,
    Runtime,
    Spawn,
    Manifest,
    Source,
}

impl MapPropertiesField {
    pub(super) const ALL: [Self; 8] = [Self::Name, Self::Id, Self::Kind, Self::Notes,
        Self::Runtime, Self::Spawn, Self::Manifest, Self::Source];

    pub(super) const fn index(self) -> usize { self as usize }
}

/// 固定身份和修订阻止稳定帧反复克隆长备注、路径及格式化正文。
#[derive(Default)]
pub(super) struct MapPropertiesObservation {
    pub(super) map_id: Option<MapId>,
    pub(super) current_map: Option<MapId>,
    pub(super) selection_pending: bool,
    pub(super) manifest: Option<bevy::asset::AssetId<feathrise_map_content::MapManifestAsset>>,
    pub(super) asset_tick: u32,
    pub(super) registry_revision: u64,
    pub(super) draft_revision: u64,
    pub(super) draft_map: Option<MapId>,
    pub(super) loading: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum SidebarField {
    #[default]
    CurrentName,
    CurrentState,
    DirectoryFeedback,
    SelectedKind,
    SelectedName,
    SelectedIdentity,
    SelectionHint,
    OperationFeedback,
    CurrentSummary,
    SelectedSummary,
}

/// 与原按钮实际处理完成后的身份绑定；待进入或确认状态变化后不保留旧等待文案。
pub(super) struct FeedbackContext {
    pub(super) selected: Option<MapBrowserItem>,
    pub(super) selection_revision: u64,
    pub(super) current_map: Option<MapId>,
    pub(super) pending_map: Option<MapId>,
    pub(super) folder_revision: u64,
    pub(super) catalog_revision: u64,
    pub(super) map_dialog: bool,
    pub(super) folder_dialog: bool,
    pub(super) switch_confirmation: bool,
    pub(super) deletion_busy: bool,
    pub(super) dirty: bool,
    pub(super) transitioning: bool,
}

#[derive(Default)]
pub(super) struct SelectionLookup {
    pub(super) initialized: bool,
    pub(super) item: Option<MapBrowserItem>,
    pub(super) projection: Option<(MapBrowserIndexSignature, u64)>,
    pub(super) source: Option<Arc<Vec<MapBrowserEntry>>>,
    pub(super) cursor: usize,
    pub(super) complete: bool,
    pub(super) record: Option<MapBrowserEntry>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum OperationPhase {
    #[default]
    Idle,
    SavingFolders,
    EditingMap,
    Creating,
    CreateAfterSave,
    Deleting,
    DeleteWaiting,
    DeleteConfirmation,
    SwitchConfirmation,
    SwitchWaiting,
    Switching,
    Dragging,
    FolderDialog,
    MapDialog,
}

#[derive(Default)]
pub(super) struct SidebarObservation {
    pub(super) initialized: bool,
    pub(super) current_map: Option<MapId>,
    pub(super) current_name: String,
    pub(super) dirty: bool,
    pub(super) saving: bool,
    pub(super) selected: Option<MapBrowserItem>,
    pub(super) selection_revision: u64,
    pub(super) selection_pending: bool,
    pub(super) record: Option<MapBrowserEntry>,
    pub(super) lookup_complete: bool,
    pub(super) rebuilding: bool,
    pub(super) projection: Option<(MapBrowserIndexSignature, u64)>,
    pub(super) total: usize,
    pub(super) projection_pending: bool,
    pub(super) catalog_revision: u64,
    pub(super) catalog_error: Option<String>,
    pub(super) folder_ready: bool,
    pub(super) blocked: Option<String>,
    pub(super) folder_error: Option<String>,
    pub(super) operation: OperationPhase,
    pub(super) action_feedback: String,
}
