//! F12右侧资产浏览器的按需二维纹理与GLB/GLTF离屏预览。
//!
//! 【功能设计】资产选择只建立预览，不创建地图摆放或预设引用；文本文档只有显式保存才写盘。图片直接交给UI；
//! Scene0在独立渲染层中加载，并按固定后代预算计算完整包围盒后自动取景；
//! GLB/GLTF子列表按固定元数据预算枚举，单primitive使用原材质并以固定顶点预算兜底取景。
//! 滚轮缩放、图片平移、模型轨道与中键平移只改临时视图；共用鼠标门防止拖出后误改地图。

use bevy::camera::visibility::VisibilitySystems; // Scene0取景必须等待Bevy补齐Mesh Aabb。
use bevy::clipboard::Clipboard; // 子资产完整引用复制复用项目唯一系统剪贴板。
use bevy::prelude::*; // App、PostUpdate与系统排序。

use crate::MapEditorSystemSet; // 加载、表现继续服从编辑器既有阶段。

mod mounting; // 应用可提交已有宿主，唯一画布仍由普通预览负责迁移与回收。
#[cfg(feature = "validation")]
pub(crate) mod rigging_validation;
mod audio; // 音频后台受检解码，单输出复用同一侧栏/中央预览宿主。
pub(crate) mod animation; // 动画GLB关键帧时间轴、骨架预览和独立草稿保存。
pub(crate) mod rigging; // 静态人形 GLB 的本地绑骨、关节点和独立动作选择。
pub(crate) mod components; // 图片/模型中央标签与原位挂载入口。
mod focus; // 浏览文件与预设展示显式切换，同一相机不会被活动草稿持续占用。
mod interaction; // Scene0/primitive选择、分页和显式完整引用复制。
mod loading; // 图片与Scene0选择、异步状态和离屏视图生命周期。
pub(crate) mod compatible_texture; // UI与缩略图PBR共用的二维采样契约和后台数值可视化。
mod messages;
mod opening; // 列表和缩略图双击都按路径打开/激活多文件标签。
pub(crate) mod documents; // 文本、源码、配置与Markdown连续排版，共用整篇草稿和显式受检保存。
mod model; // 模型专层隔离、固定预算包围盒扫描和自适应取景。
mod navigation; // 画布内开始的鼠标手势及图片/模型增量视图应用。
mod presentation; // 仅按预览状态变更原位更新固定UI节点。
pub(crate) mod resources; // 独立导航状态不触发资产重新加载或扫描。
mod scene; // 固定标题、画布、占位与状态文本。
mod sidebar; // 目录与所选资源留在侧栏，唯一预览面板仍可原位迁移。
mod subasset_presentation; // 五槽子资产行池、分页、引用及选中态同步。
mod subassets; // 根元数据有界枚举、热更新失效和单primitive加载。
pub(crate) mod types; // 预览状态、阶段和专用marker。
mod view_math; // 两种预览的缩放、轨道、平移和安全裁剪数学。
mod workspace; // 图片/模型中央页只迁移唯一画布，切标签保留轻量观察状态。
mod tab_menu; // 文件标签右键批量关闭，草稿和作者预设仍由原会话维护。
#[cfg(test)]
mod workspace_gpu_tests; // 真实PNG与GLTF加载、中央RTT和原位返回的离屏验证。

pub(crate) use resources::AssetPreviewFocus; // 资产点击与作者预览动作共享唯一来源选择。
pub(crate) use resources::AssetPreviewNavigation; // 仅向统一鼠标门与外层滚动公开只读所有权判定。
pub(crate) use resources::AssetPreviewWorkspace;
pub(crate) use audio::AudioPreviewState;
pub(crate) use animation::AnimationEditorState;
pub(crate) use types::{AssetPreviewFileKind, preview_file_kind};
pub(crate) use tab_menu::{AssetTabMenuState, AssetTabMenuInputSet};
pub(crate) use messages::OpenAssetPreview;
pub(crate) use model::include_transformed_aabb; // 缩略图复用同一世界包围盒计算。
pub(crate) use view_math::frame_thumbnail_camera; // 静态截图复用完整取景，不暴露交互会话。
pub(crate) use scene::asset_preview_panel; // 右侧资产页只依赖一个稳定Scene入口。
pub(crate) use sidebar::{asset_properties_page, assets_page};
#[cfg(test)]
pub(crate) use sidebar::install_snapshot_presentation;
pub(crate) use types::AssetPreviewState; // 摆放工具只读取当前预览的Scene0或primitive稳定目标。
pub(crate) use documents::DocumentState; // 文字编辑独占本页快捷键，格式识别由资产目录统一维护。
pub(crate) use documents::OpenProjectDocument;
pub(crate) use documents::DocumentMinimapState; // 中央小地图与世界编辑共用鼠标及滚轮所有权。
pub(crate) use documents::{AssetSearchState, asset_search_panel}; // 资产栏搜索独立后台任务，点击结果复用中央文档。
pub(crate) use documents::AssetReplaceState; // 中央替换预览视区接管滚轮，不改变原保存入口。
pub(crate) use documents::DocumentBatchSaveState; // 保存结果的局部滚轮不能穿透资产栏。
pub(crate) use documents::center_host as document_center_host;
#[cfg(test)]
pub(crate) use documents::{install_snapshot_document_tabs, set_snapshot_document_tabs_active};
pub(crate) use workspace::{center_host as asset_preview_center_host, sidebar_launch as asset_preview_sidebar_launch};

/// 独立工作台只装配原目录属性和复制路径，不在宿主创建第二个3D/音频运行时。
pub(crate) fn install_workbench_asset_sidebar(app: &mut App) {
    app.init_resource::<Clipboard>();
    sidebar::install(app);
}

/// 工程浏览器离屏夹具装配原文档系统；与工作台已安装的目录/侧栏系统保持单一实例。
#[cfg(test)]
pub(crate) fn install_project_browser_test_documents(app: &mut App) {
    documents::install(app);
}

/// 把预览生命周期安装到F12既有输入、任务与表现阶段。
pub(crate) fn install(app: &mut App) {
    compatible_texture::install(app);
    audio::install(app);
    animation::install(app);
    rigging::install(app);
    documents::install(app);
    tab_menu::install(app);
    sidebar::install(app);
    app.init_resource::<types::AssetPreviewState>()
        .init_resource::<crate::game_authoring::ExternalAssetPreviewRequest>()
        .add_message::<OpenAssetPreview>()
        .init_resource::<AssetPreviewFocus>() // 默认展示当前工作区，文件点击可明确切换来源。
        .init_resource::<AssetPreviewNavigation>() // 固定一份当前资产的临时导航状态。
        .init_resource::<AssetPreviewWorkspace>()
        .init_resource::<Clipboard>() // 右键菜单未安装的测试/最小应用也具备同一剪贴板资源契约。
        .add_systems(Update, opening::open
            .after(crate::asset_catalog::handle_asset_row_clicks)
            .in_set(MapEditorSystemSet::Input))
        .add_systems(
            Update, // 共享 UI 控件已完成本帧专业数值输入，随后才判定新的预览手势。
            (
                workspace::actions, // 先完成中央来源切换，再判定子资产和画布输入。
                interaction::handle_asset_preview_actions, // 子列表按钮先切目标并冻结本次引用身份。
                navigation::control_asset_preview,         // 随后只处理画布手势和三个导航按钮。
            )
                .chain() // 同帧primitive选择不能被旧模型导航继续应用。
                .after(AssetTabMenuInputSet) // 文件菜单先消费关闭点击，预览不能抢同一次手势。
                .after(crate::map_browser::handle_map_browser_input) // 先让文件夹模态和地图拖动完成本帧判定。
                .before(
                    super::numeric_scrub_mouse_guard::update_map_editor_numeric_scrub_mouse_guard,
                ) // 同帧把捕获发布到地图、按钮与世界笔刷共用的鼠标门。
                .in_set(MapEditorSystemSet::Input), // 必须先于外层滚动和地图相机导航。
        ) // 原始事件每帧消费，不使用run_if积压输入。
        .add_systems(
            Update,
            (
                loading::setup_asset_preview_viewport,
                workspace::sync_activity, // 离开中央标签前冻结轻量观察状态，唯一视图随后可借给作者工作台。
                workspace::restore_navigation, // 先恢复同身份的完整目标，随后由唯一加载系统计算本帧渲染资格。
                loading::sync_asset_preview_selection,
                subassets::invalidate_model_hot_reload,     // 旧编号、材质和单体实体共同换代。
                subassets::advance_model_catalog,           // 根就绪后每帧最多推进64项元数据。
                loading::poll_asset_preview_loading,
                subassets::advance_primitive_preview, // 原材质与完整边界就绪后原子生成一个实体。
                navigation::sync_asset_preview_view, // resize保留倍率/视角，仅更新固定相机与UI矩阵。
            )
                .chain()
                .after(crate::asset_catalog::poll_asset_scan) // 成功定位先发布浏览意图，加载来源与面板位置在同帧收敛。
                .after(crate::game_authoring::AssetPreviewPrepare)
                .in_set(crate::game_authoring::AssetPreviewLoading)
                .in_set(MapEditorSystemSet::Tasks),
        )
        .add_systems(
            Update,
            (
                presentation::sync_asset_preview_presentation, // 保留原有加载与失败表现。
                presentation::sync_asset_preview_controls,     // 独立回显缩放倍率、按钮与手势说明。
                subasset_presentation::sync_asset_preview_subassets, // 原位更新固定五槽、页码和完整引用。
                mounting::mount_asset_preview_panel,
                workspace::sync_presentation, // 固定入口和多个文件标签同步，不创建第二套预览。
            )
                .chain()
                .in_set(MapEditorSystemSet::Presentation), // 文案与视图使用同一帧最终导航状态。
        )
        .add_systems(
            PostUpdate,
            (
                // 取景读取已发布包围盒，下一帧再在可见性传播前公开完整模型。
                model::advance_asset_preview_bounds
                    .after(VisibilitySystems::CalculateBounds)
                    .after(VisibilitySystems::VisibilityPropagate),
                model::reveal_asset_preview.before(VisibilitySystems::VisibilityPropagate),
            ),
        )
        .add_systems(Last, model::reclaim_external_preview_roots)
        .add_observer(model::observe_asset_preview_world_ready);
}
