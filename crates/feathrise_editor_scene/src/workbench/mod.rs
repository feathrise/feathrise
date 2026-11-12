//! 原一体编辑器工作台的公共装配边界。
//!
//! 【迁移设计】这里只装配原UI实体和本地导航/布局，不安装场景加载、落盘、玩法或RTT系统。
//! 独立宿主提供不可变快照并处理意图，游戏进程继续拥有真实3D；旧SceneEditorPlugin保持原行为。
//! 因而迁移不会生成第二份面板、字段定义或主题，也不会让游戏发布包依赖独立宿主。

pub(crate) mod assets;
mod catalog;
mod components;
mod input;
mod host_pages;
mod messages;
mod presentation;
mod resources;
mod settings_access;
#[cfg(test)]
mod tests;
mod types;

pub use crate::types::EditorAction;
pub use components::{
    WorkbenchFloatingPanel, WorkbenchHeader, WorkbenchHeaderItem, WorkbenchProjectLogButton,
};
pub use presentation::WorkbenchFloatingPanels;
pub use host_pages::{
    WorkbenchCentralPageHost, WorkbenchDocumentTabs, WorkbenchHostPageState,
    WorkbenchHostPagesPlugin, WorkbenchHostTabs, WorkbenchSettingsBody, WorkbenchHostPageInput,
};
pub use messages::WorkbenchIntent;
pub use resources::{WorkbenchModel, WorkbenchProbe, WorkbenchSettingsAccess};
pub use types::{AssetBrowserProbe, WorkbenchScene};

use crate::resources::*;
use crate::ui::sidebars::SceneEditorSidebarState;
use crate::ui::vegetation_preset_editor::VegetationPresetEditorState;
use bevy::prelude::*;

/// 与文档、权重布局和 EditorUiPlugin 并列安装；轻量工作台复用原静态界面。
pub struct WorkbenchUiPlugin;

/// 宿主先发布快照，再在此阶段消费原控件；业务回执在阶段结束后交还宿主。
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WorkbenchUiSet {
    Update,
}

impl Plugin for WorkbenchUiPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<feathrise_editor_ui::EditorUiPlugin>() {
            app.add_plugins(feathrise_editor_ui::EditorUiPlugin);
        }
        app.init_resource::<WorkbenchModel>()
            .init_resource::<WorkbenchProbe>()
            .init_resource::<resources::CatalogJob>()
            .add_message::<WorkbenchIntent>()
            .init_resource::<SceneEditorState>()
            .init_resource::<SceneEditorLayoutState>()
            .init_resource::<SceneEditorNumericScrubMouseGuard>()
            .init_resource::<SceneEditorSidebarState>()
            .init_resource::<SceneEditorSettingsState>()
            .init_resource::<VegetationPresetEditorState>()
            .init_resource::<SceneBrowserIndexState>()
            .init_resource::<SceneBrowserState>()
            .init_resource::<SceneBrowserDragState>()
            .init_resource::<SceneFolderPersistence>()
            .init_resource::<SceneCreationTask>()
            .init_resource::<SceneDeletionTask>()
            .init_resource::<crate::authoring::EditorAuthoringSession>() // 只为原只读回显提供空会话，不安装仓库IO或伪造作者草稿。
            .init_resource::<EditorAssetCatalog>()
            .add_systems(Startup, crate::ui::scene::spawn_scene_editor_scene)
            .add_systems(
                Update,
                (
                    catalog::advance,
                    presentation::adopt_model,
                    assets::bind_source,
                    assets::navigate,
                    crate::asset_catalog::handle_asset_row_clicks,
                    input::forward_actions,
                    crate::ui::sidebars::handle_editor_sidebar_input,
                    crate::ui::layout::handle_editor_layout_input,
                    crate::ui::layout::remember_editor_layout_preferences,
                )
                    .chain()
                    .in_set(WorkbenchUiSet::Update),
            )
            .add_systems(
                PostUpdate,
                presentation::measure.after(bevy::ui::UiSystems::PostLayout),
            )
            .add_systems(
                PostUpdate,
                crate::ui::sidebars::sync_asset_reveal_scroll
                    .after(bevy::ui::UiSystems::PostLayout)
                    .after(bevy::transform::TransformSystems::Propagate), // 连续目录定位先滚到绝对条目，再由可见行池完成实际布局。
            );
        crate::ui::theme::install(app);
        crate::ui::scene_sidebar::install_workbench_menu(app);
        crate::ui::status_bar::install(app);
        crate::ui::context_help::install(app);
        crate::ui::vegetation_preset_editor::install_workbench_presentation(app);
        crate::ui::asset_preview::install_workbench_asset_sidebar(app);
        app.add_systems(
            Update,
            (
                crate::asset_catalog::start_asset_scan,
                crate::asset_catalog::poll_asset_scan,
            )
                .chain()
                .in_set(crate::SceneEditorSystemSet::Tasks),
        );
        // 后台完整采用后，同帧更新容器高度、固定行绑定和布局。不能只更新行而沿用上一目录的裁剪框；
        // PostLayout回报的“就绪”必须对应当前真实命中区域，立即点击新目录下方的文件也应有效。
        app.add_systems(
            Update,
            (
                crate::ui::sidebars::sync_editor_sidebar_presentation,
                crate::ui::layout::sync_editor_layout,
                crate::ui::rows::spawn_dynamic_rows,
                crate::ui::rows::sync_dynamic_list_layouts,
                crate::ui::systems::scroll_editor_side_panels, // 本帧滚轮先更新唯一宿主，行池随后按新位置换绑。
                crate::ui::rows::sync_scene_rows,
                crate::ui::rows::sync_asset_rows,
                crate::scene_browser::sync_scene_browser_root_row,
                presentation::sync_pages,
            )
                .chain()
                .in_set(crate::SceneEditorSystemSet::Presentation),
        );
        app.add_systems(
            Update,
            crate::ui::world_session::sync_editor_world_controls
                .in_set(crate::SceneEditorSystemSet::Presentation),
        );
        app.configure_sets(
            Update,
            (
                crate::SceneEditorSystemSet::Input,
                WorkbenchUiSet::Update,
                crate::SceneEditorSystemSet::Tasks,
                crate::SceneEditorSystemSet::Presentation,
            )
                .chain(),
        );
    }
}
