//! 场景侧栏仅保留本域显式动作的原诊断，不借用其他工具可能覆盖的全局底栏。

use super::types::FeedbackContext;
use crate::authoring::EditorAuthoringSession;
use crate::components::{SceneEditorSceneRow, SceneEditorSceneTreeScroll};
use crate::resources::{
    SceneBrowserDragState, SceneBrowserIndexState, SceneBrowserState, SceneCreationTask, SceneDeletionTask,
    SceneEditorNumericScrubMouseGuard, SceneEditorState, SceneFolderPersistence,
};
use crate::ui::sidebars::SceneEditorSidebarState;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use feathrise_scene_content::{SceneContentRegistry, SceneManifestAsset};

/// 五个目录树图片句柄在创建原行池时加载一次，不随场景数量增加。
#[derive(Resource, Clone)]
pub(crate) struct SceneBrowserTreeIcons {
    pub(crate) folder: Handle<Image>,
    pub(crate) scene: Handle<Image>,
    pub(crate) root: Handle<Image>,
    pub(crate) expand: Handle<Image>,
    pub(crate) collapse: Handle<Image>,
}
impl SceneBrowserTreeIcons {
    pub(crate) fn load(server: &AssetServer) -> Self {
        Self { folder: server.load("icons/editor/tree_folder.png"), scene: server.load("icons/editor/tree_scene.png"),
            root: server.load("icons/editor/tree_root.png"), expand: server.load("icons/editor/tree_expand.png"),
            collapse: server.load("icons/editor/tree_collapse.png") }
    }
}

#[derive(Resource, Default)]
pub(crate) struct SceneSidebarFeedback {
    pub(super) context: Option<FeedbackContext>,
    pub(super) message: String,
}

/// 原输入入口借用本页身份与反馈；可选资源兼容没有侧栏的原域测试。
#[derive(SystemParam)]
pub(crate) struct SceneSidebarInputContext<'w> {
    pub(crate) feedback: Option<ResMut<'w, SceneSidebarFeedback>>,
    pub(crate) sidebar: Option<ResMut<'w, SceneEditorSidebarState>>,
    layout: Option<Res<'w, crate::resources::SceneEditorLayoutState>>,
}

impl SceneSidebarInputContext<'_> {
    pub(crate) fn visible(&self) -> bool {
        self.sidebar
            .as_ref()
            .is_none_or(|state| state.scene_browser_visible())
            && self.layout.as_ref().is_none_or(|state| state.content_shown())
    }

    /// 右侧场景属性即使左导航已换页，仍可使用当前明确显示的编辑入口。
    pub(crate) fn scene_properties_visible(&self) -> bool {
        self.sidebar.as_ref().is_some_and(|state| {
            state.right == crate::ui::sidebars::EditorRightSidebarTab::SceneEntry
        }) && self.layout.as_ref().is_none_or(|state| state.inspector_shown())
    }

    pub(crate) fn reveal_scene_entry_properties(&mut self) {
        if let Some(state) = self.sidebar.as_deref_mut() {
            state.reveal_scene_entry_properties();
        }
    }
}

/// 只读借用原状态和固定行池；不会在表现路径读取catalog数组或磁盘文件。
#[derive(SystemParam)]
pub(super) struct SidebarSources<'w, 's> {
    pub(super) hosted: Option<Res<'w, crate::workbench::WorkbenchModel>>, // 新宿主只开放已接通的真实动作；一体入口保持原资格。
    pub(super) editor: Res<'w, SceneEditorState>,
    pub(super) browser: Res<'w, SceneBrowserState>,
    pub(super) index: Res<'w, SceneBrowserIndexState>,
    pub(super) drag: Res<'w, SceneBrowserDragState>,
    pub(super) sidebar: Res<'w, SceneEditorSidebarState>,
    pub(super) session: Res<'w, EditorAuthoringSession>,
    pub(super) registry: Option<Res<'w, SceneContentRegistry>>,
    pub(super) manifests: Option<Res<'w, Assets<SceneManifestAsset>>>,
    pub(super) persistence: Res<'w, SceneFolderPersistence>,
    pub(super) creation: Res<'w, SceneCreationTask>,
    pub(super) deletion: Res<'w, SceneDeletionTask>,
    pub(super) mouse_guard: Res<'w, SceneEditorNumericScrubMouseGuard>,
    pub(super) feedback: Res<'w, SceneSidebarFeedback>,
    pub(super) menu: Option<Res<'w, super::SceneDirectoryMenuState>>,
    pub(super) rows: Query<'w, 's, &'static SceneEditorSceneRow>,
    pub(super) scroll: Query<'w, 's, &'static ScrollPosition, With<SceneEditorSceneTreeScroll>>,
}
