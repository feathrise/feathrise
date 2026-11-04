//! 地图侧栏仅保留本域显式动作的原诊断，不借用其他工具可能覆盖的全局底栏。

use super::types::FeedbackContext;
use crate::authoring::EditorAuthoringSession;
use crate::components::{MapEditorMapRow, MapEditorMapTreeScroll};
use crate::resources::{
    MapBrowserDragState, MapBrowserIndexState, MapBrowserState, MapCreationTask, MapDeletionTask,
    MapEditorNumericScrubMouseGuard, MapEditorState, MapFolderPersistence,
};
use crate::ui::sidebars::MapEditorSidebarState;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use feathrise_map_content::{MapContentRegistry, MapManifestAsset};

/// 五个目录树图片句柄在创建原行池时加载一次，不随地图数量增加。
#[derive(Resource, Clone)]
pub(crate) struct MapBrowserTreeIcons {
    pub(crate) folder: Handle<Image>,
    pub(crate) map: Handle<Image>,
    pub(crate) root: Handle<Image>,
    pub(crate) expand: Handle<Image>,
    pub(crate) collapse: Handle<Image>,
}
impl MapBrowserTreeIcons {
    pub(crate) fn load(server: &AssetServer) -> Self {
        Self { folder: server.load("icons/editor/tree_folder.png"), map: server.load("icons/editor/tree_map.png"),
            root: server.load("icons/editor/tree_root.png"), expand: server.load("icons/editor/tree_expand.png"),
            collapse: server.load("icons/editor/tree_collapse.png") }
    }
}

#[derive(Resource, Default)]
pub(crate) struct MapSidebarFeedback {
    pub(super) context: Option<FeedbackContext>,
    pub(super) message: String,
}

/// 原输入入口借用本页身份与反馈；可选资源兼容没有侧栏的原域测试。
#[derive(SystemParam)]
pub(crate) struct MapSidebarInputContext<'w> {
    pub(crate) feedback: Option<ResMut<'w, MapSidebarFeedback>>,
    pub(crate) sidebar: Option<ResMut<'w, MapEditorSidebarState>>,
    layout: Option<Res<'w, crate::resources::MapEditorLayoutState>>,
}

impl MapSidebarInputContext<'_> {
    pub(crate) fn visible(&self) -> bool {
        self.sidebar
            .as_ref()
            .is_none_or(|state| state.map_browser_visible())
            && self.layout.as_ref().is_none_or(|state| state.content_shown())
    }

    /// 右侧地图属性即使左导航已换页，仍可使用当前明确显示的编辑入口。
    pub(crate) fn map_properties_visible(&self) -> bool {
        self.sidebar.as_ref().is_some_and(|state| {
            state.right == crate::ui::sidebars::EditorRightSidebarTab::Map
        }) && self.layout.as_ref().is_none_or(|state| state.inspector_shown())
    }

    pub(crate) fn reveal_map_properties(&mut self) {
        if let Some(state) = self.sidebar.as_deref_mut() {
            state.reveal_map_properties();
        }
    }
}

/// 只读借用原状态和固定行池；不会在表现路径读取catalog数组或磁盘文件。
#[derive(SystemParam)]
pub(super) struct SidebarSources<'w, 's> {
    pub(super) hosted: Option<Res<'w, crate::workbench::WorkbenchModel>>, // 新宿主只开放已接通的真实动作；一体入口保持原资格。
    pub(super) editor: Res<'w, MapEditorState>,
    pub(super) browser: Res<'w, MapBrowserState>,
    pub(super) index: Res<'w, MapBrowserIndexState>,
    pub(super) drag: Res<'w, MapBrowserDragState>,
    pub(super) sidebar: Res<'w, MapEditorSidebarState>,
    pub(super) session: Res<'w, EditorAuthoringSession>,
    pub(super) registry: Option<Res<'w, MapContentRegistry>>,
    pub(super) manifests: Option<Res<'w, Assets<MapManifestAsset>>>,
    pub(super) persistence: Res<'w, MapFolderPersistence>,
    pub(super) creation: Res<'w, MapCreationTask>,
    pub(super) deletion: Res<'w, MapDeletionTask>,
    pub(super) mouse_guard: Res<'w, MapEditorNumericScrubMouseGuard>,
    pub(super) feedback: Res<'w, MapSidebarFeedback>,
    pub(super) menu: Option<Res<'w, super::MapDirectoryMenuState>>,
    pub(super) rows: Query<'w, 's, &'static MapEditorMapRow>,
    pub(super) scroll: Query<'w, 's, &'static ScrollPosition, With<MapEditorMapTreeScroll>>,
}
