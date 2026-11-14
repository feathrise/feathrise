//! 正式场景作者工作台；独立编辑器常规安装，原游戏可按需装配。
//!
//! 场景作者文件写入`assets/scenes`；资产文本编辑仅在显式保存时写回当前assets内源文件。
//! 正式运行时继续通过原AssetServer热加载路径读取同一份数据。

mod asset_catalog; // 有界后台扫描assets目录，不在帧关键路径同步遍历磁盘。
mod authoring; // 新建场景、原子文件写入与Rust白模GLB构建。
mod components; // UI、离屏视口和作者预览实体marker。
mod constants; // 新建场景尺寸默认值、输入范围和地形采样预算。
mod editor_monitor; // F4直接复刻F3分页外壳，首版只接入当前作者场景的树木正式数据。
mod scene_browser; // 编辑器虚拟文件夹、导航、拖拽与独立归类文件事务。
mod scene_identity; // 目录选择、切换场景和删除共用真实玩法SceneId解析。
mod scene_residency; // 作者派生实体、相机和GPU预览的完整场景驻留边界。
mod monitor_resources; // F4两级分页、显隐与半秒刷新状态独立于F3运行时资源面板。
mod resources; // 编辑会话、资产目录和后台任务槽。
mod types; // 工具、视图、动作和后台结果普通类型。
mod ui; // BSN静态布局、固定动态行池与交互系统。
mod viewport; // 独立RTT相机与透视/正交CAD式导航。
mod world_environment_controls; // 从 F1 迁入的时间、月份和运行时天气调试控件。
pub mod workbench; // 原工作台的独立装配入口；新旧宿主共用实体树，作者/运行时系统按边界分别接入。
pub mod standalone; // 首版独立编辑模式只请求运行已保存工程，作者World与历史继续驻留本地。
pub mod game_authoring; // 应用自有作者域只通过来源、状态及挂载合同接入，不反向依赖原游戏实现。

use bevy::prelude::*; // App、Plugin、Startup与Update。
use feathrise_atmosphere::{
    WeatherAuthoringPreview, WeatherCommand, WeatherDebugOverride, WeatherState, WeatherSystemSet,
    WorldClock, WorldTimeSystemSet,
}; // F12输入必须先于世界推进与天气解析，回显等待最终光照和天气效果。
use feathrise_editor_ui::EditorUiSystemSet;
use feathrise_scene_core::input::{GameCursorSystemSet, GamePauseSystemSet}; // F12暂停与世界笔刷鼠标分别进入公共唯一应用阶段。
use feathrise_scene_content::{ActiveDynamicScene, DynamicCelestialLightingPreview}; // 保存交接与 F12 未保存天体预览复用共享稳定身份。
use feathrise_scene::{
    DynamicTerrainHeightStatus, DynamicWaterStatus, ScenePlacementRebuildRequest,
    ScenePlacementRuntimeStatus, ScenePortalRuntimeStatus, TerrainGridPreview, TerrainShadingPreview,
    WaterGpuUploadQueue, WaterGpuUploadReceipts,
}; // 保存交接读取水域/摆放正式证据，预览与正式水共享GPU登记资源。
use feathrise_scene_core::residency::{
    SceneResidencyActivity, SceneResidencyResourcePlugin, SceneResidencyRuntimeSet,
};
use feathrise_editor_core::EditorSettings; // 独立编辑器测试也要提供全部数值拖动灵敏度默认资源。
use feathrise_scene_core::state::ScenePhase; // 作者预览跟随当前场景加载与退出阶段回收。

use asset_catalog::*; // 资产扫描启动、轮询和点击系统。
use authoring::*; // 新场景任务完成采用系统。
use scene_browser::*; // 虚拟场景文件夹加载、输入、索引与Presentation系统。
use resources::*; // 插件初始化全部会话资源。
use ui::actions::*; // 工具按钮、快捷键和选中态回显。
use ui::brush_controls::{
    BrushNumericInputState, configure_brush_slider_focus, handle_brush_numeric_inputs,
    sync_brush_slider_interaction_styles, sync_brush_sliders,
}; // 中央数值框提供精确输入、实际值左拖和独立灵敏度右拖。
use ui::close::finish_scene_editor_close; // 未保存地形的画面/碰撞联合关闭门。
use ui::delete_scene_dialog::{configure_delete_scene_dialog_focus, sync_delete_scene_dialog}; // 可恢复删除确认框补齐模态焦点并做增量显隐和文字回显。
use ui::environment::sync_editor_environment_text; // 天空盒草稿与统一加载阶段回显。
use ui::gltf_browser::{
    EditorGltfBrowserState, handle_gltf_browser_input, refresh_gltf_browser_cache,
    spawn_gltf_browser_rows, sync_gltf_browser_page_button_styles, sync_gltf_browser_rows,
}; // 已摆放Scene0树按Input、Tasks、Presentation三阶段保持稳定选择。
use ui::layer_status::sync_editor_layer_status; // 地表与植被页面分别回显当前作者选择。
use ui::scene_deletion::handle_scene_deletion_buttons; // 删除按钮执行保护校验，只在目标已退出的稳定世界或空工作台回收。
use ui::scene_transition::drive_pending_scene_transition; // 创建成功或显式点击进入后等待清单并自动进入目标。
use ui::new_scene_dialog::{scroll_new_scene_dialog_body, sync_new_scene_dialog_status}; // 资料正文有界滚动，底部操作固定可见，确认后再后台采用。
use ui::numeric_scrub_mouse_guard::{
    scene_editor_numeric_scrub_mouse_available, update_scene_editor_numeric_scrub_mouse_guard,
}; // 数值拖动及其和弦release帧统一拥有全部场景编辑器鼠标入口。
use ui::placement_transform::{
    PlacementTransformInputState, apply_selected_placement_relocation,
    handle_placement_transform_actions, handle_placement_transform_numeric_input,
    setup_placement_transform_numeric_inputs, sync_placement_transform_panel,
}; // 完整资产变换、复制、删除与视口落点共用正式placements草稿和历史。
use ui::placement_water::{
    PlacementInstanceIndex, PlacementWaterNumericInputState, PlacementWaterRuntimeSelectionCache,
    PlacementWaterSelectionCache, apply_placement_water_numeric_scrub,
    handle_placement_water_actions, handle_placement_water_numeric_input,
    observe_placement_water_status_inserted, observe_placement_water_status_removed,
    refresh_placement_water_runtime_selection_cache, refresh_placement_water_selection_cache,
    register_placement_instance_root, setup_placement_water_numeric_inputs,
    sync_placement_water_button_styles, sync_placement_water_numeric_inputs,
    sync_placement_water_runtime_status, sync_placement_water_text,
    unregister_placement_instance_root,
}; // Blender整网格赋水进入统一草稿、撤销与实时正式材质覆盖。
use ui::portal_list::*; // 当前场景源点与场景树目标落点完整双列表的输入和Presentation。
use ui::rows::*; // 固定场景/资产行池。
use ui::scene::spawn_scene_editor_scene; // BSN全屏静态场景。
use ui::sidebars::*; // 八页侧栏会话状态、页签输入和上下文Presentation。
use ui::systems::*; // F12、按钮、文本和显隐同步。
use ui::terrain_conflict_dialog::{
    configure_terrain_conflict_dialog_focus, handle_terrain_conflict_buttons,
    sync_terrain_conflict_dialog,
}; // 外部GLB冲突在独立模态层冻结精确选择。
use ui::terrain_shading::{
    handle_scene_base_terrain_row, handle_terrain_grid_button, handle_terrain_shading_buttons,
    sync_terrain_grid_button_style, sync_terrain_shading_button_styles,
    sync_terrain_shading_session,
}; // 场景基础地形选择、双模式着色与独立网格按钮只更新本次运行的显示状态。
use ui::vegetation_layers::*; // 作者图层结构按钮、固定行池与正式profile选择。
use ui::vegetation_presets::sync_vegetation_preset_list; // 全目录预设在滚动侧栏中按三列连续显示。
use ui::water_feature_list::*; // 静水、河流与瀑布统一固定行池和稳定ID直选。
use ui::water_layers::{PendingClipmapWaterLayerCreate, handle_water_layer_buttons}; // 独立水位图层结构操作与唯一待放置请求。
use ui::water_level_input::{
    handle_water_level_input, setup_water_level_input, sync_water_level_input,
}; // 公共数字输入冻结场景、修订和稳定水层，并进入现有撤销与GPU预览链路。
use viewport::*; // 原生ECS离屏视口系统。
use viewport::placement_move::{PlacementMoveState, handle_placement_move, draw_placement_move}; // 手柄沿用作者事务与唯一视口层。
use world_environment_controls::*; // 环境页运行时世界控制组。

pub(crate) use feathrise_editor_core::SceneEditorSystemSet; // 稳定系统阶段由公共契约唯一拥有。

/// 场景作者工具插件；是否随游戏发布由游戏宿主决定，正式独立编辑器默认使用。
/// 宿主并列装配文档、权重布局与 GLB 后台插件，提供静态场景工厂和后台操作。
#[derive(Default)]
pub struct SceneEditorPlugin;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct SceneDocumentTransitionSet; // 完整文档/场景交接独立于被冻结的作者派生任务。

/// 独立安装F12插件时也完整提供摆放保存系统参数；与scene插件重复初始化保持幂等。
fn init_placement_save_runtime_contracts(app: &mut App) {
    app.init_resource::<ScenePlacementRuntimeStatus>()
        .add_message::<ScenePlacementRebuildRequest>(); // 原样失败重试通过消息进入scene既有候选链，不伪造Asset Modified。
}

impl Plugin for SceneEditorPlugin {
    fn build(&self, app: &mut App) {
        #[cfg(test)]
        app.insert_resource(feathrise_editor_project_browser::directory_operations());
        if !app.is_plugin_added::<feathrise_editor_core::EditorSettingsPlugin>() {
            app.add_plugins(feathrise_editor_core::EditorSettingsPlugin);
        }
        if !app.is_plugin_added::<SceneResidencyResourcePlugin>() {
            app.add_plugins(SceneResidencyResourcePlugin);
        }
        app.world_mut()
            .resource_mut::<SceneResidencyActivity>()
            .enabled = true;
        app.init_resource::<authoring::AuthoringRetirement>()
            .init_resource::<game_authoring::scene_interaction::SceneInteractionState>()
            .add_systems(Update, authoring::poll_authoring_retirement.in_set(SceneEditorSystemSet::Tasks));
        scene_residency::install(app); // 相机与派生预览移交场景槽，作者源仍由文档持有。
        app.configure_sets(Update, (TerrainLiveSet::Adopt, TerrainLiveSet::React).chain());
        if !app.is_plugin_added::<feathrise_editor_ui::EditorUiPlugin>() {
            app.add_plugins(feathrise_editor_ui::EditorUiPlugin); // 独立 F12 也安装作者控件；底层自动与游戏去重。
        }
        ui::text_input::install(app); // 先安装官方输入的公共消息保护与静态字段绑定。
        ui::scene_objects::install(app); // 场景对象管理独立推进，关闭与显隐恢复不被自己的作者输入门阻止。
        ui::theme::install(app); // 偏好只更新小色板，保留已有UI、输入焦点、草稿和布局。
        ui::terrain_shading::install_collision_preview(app); // 复用全局物理显示偏好，不新增碰撞遍历或作者状态。
        viewport::modes::install(app); // 全局模式先于命中收口，选择模式禁止任何作者落笔。
        viewport::tool_preview::install(app);
        viewport::shading::install(app); // 着色入口、相机参数和按视图隔离的渲染显示。
        app.add_plugins(viewport::snapping::grid_render::GridRenderPlugin); // 全部着色共用按视图解析的无限细线网格。
        viewport::spatial_brush::install(app);
        authoring::volume_sculpt::install(app); // 新原型复用既有F12阶段，默认不进入发布构建。
        authoring::basic_shapes::install(app); // 统一网格创建意图和立方体预览遵守原输入、地面查询及释放点击阶段。
        authoring::mesh_edit::install(app); // 点边面编辑独占局部几何输入，退出后恢复完整对象工作流。
        init_placement_save_runtime_contracts(app); // 必须早于安装start/poll save系统，独立插件首帧也不能缺SystemParam。
        app.init_gizmo_group::<SceneEditorBrushGizmos>()
            .init_gizmo_group::<viewport::snapping::EditorGridGizmos>()
            .init_gizmo_group::<viewport::snapping::EditorGridOutlineGizmos>() // 只为真实落点十字保留深边与浅芯。
            .init_resource::<WeatherAuthoringPreview>() // 独立编辑器测试未安装完整天气插件时仍使用安全无覆盖默认值。
            .init_resource::<WorldClock>() // F12 独立安装仍能直接编辑安全默认世界时钟。
            .init_resource::<WeatherState>() // 环境页状态摘要始终有合法自然天气来源。
            .init_resource::<WeatherDebugOverride>() // 天气模式与过渡只进入运行时调试覆盖。
            .add_message::<WeatherCommand>() // 立即应用和重新随机复用天气核心唯一命令入口。
            .init_resource::<EditorSettings>() // 独立测试未安装完整设置插件时仍具备一倍拖动灵敏度。
            .init_resource::<viewport::snapping::MeshSnapCursor>()
            .init_resource::<viewport::water_plane::WaterPlaneCursor>() // 无地形时也能按当前水位绘制静水。
            .init_resource::<TerrainShadingPreview>() // F12单独安装时也从平滑着色开始。
            .init_resource::<TerrainGridPreview>() // F12单独安装时也从关闭网格开始。
            .init_resource::<TerrainShadingSession>() // 切换场景恢复各图着色与网格，关闭F12仍保留玩家视角设置。
            .init_resource::<SceneEditorState>()
            .init_resource::<feathrise_scene_core::input::InputGateState>()
            .init_resource::<feathrise_scene_core::input::EditorInputState>()
            .init_resource::<PlacementMoveState>() // 只有一个手柄拖动槽，不进入场景或偏好保存。
            .init_resource::<authoring::placement_batch::PlacementBatchState>() // 剪贴板跨标签保留，批量操作只有一个CPU槽。
            .init_resource::<viewport::multi_select::PlacementSelectionSet>() // 普通资产多选仅属于当前编辑会话。
            .init_resource::<SceneEditorViewMemory>() // 只记每个场景的轻量相机元数据，跨切换场景RTT回收保留。
            .init_resource::<SceneEditorSettingsState>()
            .init_resource::<SurfaceStrokeTrail>() // 地表即时路径只有固定容量，不进入持久化。
            .init_resource::<SurfaceSelectionOverlay>() // 当前选中作者层的固定预算辅助图。
            .init_resource::<PortalRowEntityRevision>() // 传送完整列表跨帧补行时用结构修订唤醒Presentation。
            .init_resource::<SelectedWaterViewportState>() // 三类水域对象的高亮与双击定位只保留有限会话缓存。
            .init_resource::<SelectedPortalViewportState>() // 当前源传送点单击标记与双击定位只读取作者小型记录。
            .init_resource::<SelectedPlacementViewportState>() // Scene0网格选择只缓存当前活动根和有限primitive。
            .init_resource::<EditorViewportSelectionState>() // 视口植被选择只保存正式输出身份和小型重叠候选序列。
            .init_resource::<LayerCandidateState>() // 跨类别作者层候选使用一个在途后台任务与稳定ID。
            .init_resource::<viewport::source_fence::SourceAuthoringFence>() // 子场景范围索引与析构共用单飞预算。
            .init_asset::<types::SceneFolderAsset>() // 编辑器专用归类JSON不进入运行时公共场景接口。
            .init_asset_loader::<SceneFolderAssetLoader>() // 只接管复合扩展`.scene_folders.json`。
            .init_resource::<SceneBrowserState>() // 首载前禁写，缺失文件事件后开放空分类。
            .init_resource::<SceneFolderPersistence>() // 归类结构修改使用唯一IoTaskPool写入槽。
            .init_resource::<SceneBrowserIndexState>() // 大目录分帧快照、后台完整树与展开投影原子采用。
            .init_resource::<SceneBrowserDragState>() // 六物理像素阈值及release帧鼠标隔离。
            .init_resource::<SceneEditorSidebarState>() // 页签、帮助和滚动仅保留在当前进程，不进入settings或场景文件。
            .init_resource::<SceneEditorLayoutState>() // 首选宽度和显隐持久化，拖动与专注模式留在本进程。
            .init_resource::<BrushNumericInputState>() // 三个笔刷字段冻结raw起点并隔离模态/外部换代后的迟到Changed。
            .init_resource::<SceneEditorNumericScrubMouseGuard>() // 拖动结束后延迟到三键全松后的静默帧再归还鼠标。
            .add_systems(
                PreUpdate,
                ui::numeric_scrub_mouse_guard::track_floating_panel_pointer.after(bevy::ui::ui_focus_system),
            )
            .init_resource::<EditorAssetCatalog>()
            .init_resource::<SceneCreationTask>()
            .init_resource::<SceneDeletionTask>() // 删除确认、切换场景等待与目录移动保持单一事务。
            .init_resource::<EditorAuthoringSession>()
            .init_resource::<ui::scene_documents::SceneDocuments>() // 多场景草稿只在本进程保留，后台标签不创建运行时视图。
            .init_resource::<resources::SceneLoadingRecovery>()
            .init_resource::<feathrise_scene_content::SceneAuthorPreview>() // 只有活动标签的完整临时输入参与正式场景加载。
            .init_resource::<PlacementTerrainFollowState>() // 地形修订后按固定预算让跟随型显式资产重新贴地。
            .init_resource::<GlobalEnvironmentAuthoringState>() // 全局文件草稿与场景草稿保持独立脏状态和共同撤销线。
            .init_resource::<GlobalEnvironmentSaveTask>() // 仅全局脏时不进入任何场景预检。
            .init_resource::<DynamicCelestialLightingPreview>() // 独立编辑器测试也具备安全空预览资源。
            .init_resource::<EditorGltfBrowserState>() // 强持有唯一Scene0并公开稳定网格选择。
            .init_resource::<PlacementTransformInputState>() // 七字段事务与一次视口移动模式共享稳定作者身份。
            .init_resource::<PlacementWaterSelectionCache>() // 属性文字、按钮和NumericInput共享变化帧派生绑定。
            .init_resource::<PlacementWaterNumericInputState>() // 两个字段共享唯一字符绑定和左拖作者事务。
            .init_resource::<PlacementInstanceIndex>() // 连续作者写入按稳定ID定位少量并存placement根。
            .init_resource::<PlacementWaterRuntimeSelectionCache>() // Pending/Ready/Failed只定向聚合当前选择。
            .init_resource::<DraftLoadTask>()
            .init_resource::<DraftSaveTask>()
            .init_resource::<PlacementWaterEvidenceRegistry>() // marker和状态生命周期增量索引替代保存等待帧全量Query。
            .init_resource::<WaterFileHandoff>() // 保存后强持有本次脏R8，并等待完整动态水新代次再允许关闭。
            .init_resource::<PortalPreviewHandoff>() // 正式传送图严格新Ready前持续保留刚保存的可见预览。
            .init_resource::<ScenePortalRuntimeStatus>() // 独立编辑器测试未安装scene插件时仍提供安全Waiting证据。
            .init_resource::<ActiveDynamicScene>() // 允许独立编辑器应用安全构造保存系统参数，正式插件初始化保持幂等。
            .init_resource::<DynamicWaterStatus>() // 同上；完整场景插件会在同一资源上发布真实运行时证据。
            .init_resource::<feathrise_scene::DynamicClipmapWaterDraft>() // Clipmap未保存静水整批交给正式共享环带后端。
            .init_resource::<WaterGpuUploadQueue>() // 独立编辑器测试使用立即回执；完整场景插件安装RenderApp监控。
            .init_resource::<WaterGpuUploadReceipts>()
            .init_resource::<TerrainBrushStroke>()
            .init_resource::<SparseTerrainAuthoringState>() // 新图清单、局部块与后台单飞任务不进入旧GLB草稿。
            .init_resource::<feathrise_scene_content::SparseHeightLiveOverride>() // 独立F12插件也能把未保存局部块实时交给场景消费者。
            .init_resource::<TerrainPreviewState>()
            .init_resource::<TerrainLiveApplyState>()
            .init_resource::<TerrainExternalChangeState>()
            .init_resource::<DynamicTerrainHeightStatus>() // 独立编辑器测试没有scene插件时仍有安全等待状态。
            .init_resource::<SurfaceBrushStroke>()
            .init_resource::<SurfacePreviewState>()
            .init_resource::<PuddleAuthoringState>() // 单在途积水后台操作与完整历史。
            .init_resource::<feathrise_scene_content::ScenePuddleSaveHandoff>() // 保存后保持作者画面直到正式R8内容证明完成。
            .init_resource::<feathrise_scene_content::ScenePuddleLiveOverride>() // 独立编辑器同样提供安全空覆盖。
            .init_resource::<VegetationGroundCursor>() // 精确地面任务有界且关闭后不泄漏工作。
            .init_resource::<VegetationBrushStroke>()
            .init_resource::<VegetationBlankLayerAllocation>() // 任意分辨率空白R8在唯一后台槽清零后才原子加入草稿。
            .init_resource::<VegetationCompileState>() // 多作者图层只产生一个有界后台正式层编译任务。
            .init_resource::<VegetationPreviewState>()
            .init_resource::<VegetationNumericInputState>() // 五项参数左拖只在松手形成一次撤销与编译请求。
            .init_resource::<WaterBrushStroke>()
            .init_resource::<PendingClipmapWaterLayerCreate>() // 侧栏新建后等待真实Clipmap三角命中，不回退水平面。
            .init_resource::<WaterMaskMaterializeTask>() // 共享大R8首次写侧在后台复制，输入帧只非阻塞轮询。
            .init_resource::<WaterLevelScrubState>() // 水位左拖实时预览但只在松开形成一条撤销。
            .init_resource::<WaterNetworkEditState>() // 河流与瀑布拖点实时预览但松手只压一条完整快照撤销。
            .init_resource::<WaterNetworkNumericInputState>() // 四项点参数左拖只在松手重建一次完整水面。
            .init_resource::<WaterPreviewState>()
            .init_resource::<BoundaryEditState>()
            .init_resource::<BoundaryThicknessInputState>() // 墙厚左拖只在松手重建一次Avian静态碰撞。
            .init_resource::<BoundaryLiveOverrideState>()
            .init_resource::<EnvironmentNumericInputState>() // 四项天空场景属性共享唯一数字指针状态。
            .init_resource::<CelestialNumericInputState>() // 全局与场景二十个标量共享唯一字符焦点和左拖事务。
            .init_resource::<CelestialSectionState>() // 两个天体设置折叠区只保存当前编辑会话显隐。
            .init_resource::<VegetationDensityOverlayState>()
            .init_resource::<LiveBladeDensityState>()
            .init_resource::<LiveClipmapWaterState>() // 单后台任务、每帧一张R8后原子发布静水草稿。
            .init_resource::<EditorBladeViewHandoff>()
            .add_observer(register_placement_water_evidence_root) // 活动、候选和作者预览根新增时O(1)登记。
            .add_observer(unregister_placement_water_evidence_root) // marker删除或实体despawn立即撤销旧Ready聚合。
            .add_observer(register_placement_instance_root) // 正式、候选和预览根Add时建立作者写入索引。
            .add_observer(unregister_placement_instance_root) // Remove或despawn立即撤销旧实体信用。
            .add_observer(authoring::placement_batch::retire_empty_paste_group) // 原子显示的粘贴父级在失去最后一个孩子时回收。
            .add_observer(observe_placement_water_status_inserted) // 状态插入/替换只推进所属placement票据。
            .add_observer(observe_placement_water_status_removed) // 状态移除立即撤销旧回显信用。
            .add_observer(observe_added_placement_water_status) // 首次Pending/Ready/Failed只唤醒对应实体。
            .add_observer(observe_replaced_placement_water_status) // 运行时状态替换进入固定容量事件环。
            .add_observer(observe_removed_placement_water_status) // 状态移除不能遗留旧Ready信用。
            .configure_sets(
                Update,
                (
                    SceneEditorSystemSet::Input
                        .after(EditorUiSystemSet::NumericInput) // Enter或失焦提交同帧转成水位作者请求。
                        .before(WorldTimeSystemSet::Advance) // F12时刻与月份输入在同帧自动推进前生效。
                        .before(WeatherSystemSet::Advance) // 天气模式与命令在自动状态机推进前生效。
                        .before(WeatherSystemSet::Resolve), // 同帧按钮变化进入光照、风、云、雨和雾解析。
                    game_authoring::DomainInputSettled.after(SceneEditorSystemSet::Input),
                    SceneDocumentTransitionSet.after(game_authoring::DomainInputSettled),
                    SceneResidencyRuntimeSet.after(SceneDocumentTransitionSet),
                    SceneEditorSystemSet::Tasks
                        .after(SceneDocumentTransitionSet)
                        .run_if(scene_residency::document_active),
                    SceneEditorSystemSet::Cursor
                        .after(SceneEditorSystemSet::Tasks)
                        .after(GamePauseSystemSet::Apply) // blocker先收口，再根据本帧最终视口命中决定系统鼠标所有权。
                        .before(GameCursorSystemSet::Apply)
                        .before(WeatherSystemSet::Resolve),
                    SceneEditorSystemSet::Presentation
                        .after(SceneEditorSystemSet::Cursor)
                        .after(GameCursorSystemSet::Apply) // 系统鼠标和世界笔刷先完成同帧二选一，再提交本帧Gizmo。
                        .before(WeatherSystemSet::Resolve), // 离屏相机与关闭门先完成，再同帧解析光照、风、云、雨和雾。
                    game_authoring::DomainGroundCursor
                        .after(game_authoring::BaseGroundCursorResolved)
                        .before(game_authoring::EditorCursorPresentation)
                        .in_set(SceneEditorSystemSet::Cursor),
                    game_authoring::DomainViewportInput
                        .run_if(scene_editor_numeric_scrub_mouse_available)
                        .run_if(scene_browser_general_input_available)
                        .in_set(SceneEditorSystemSet::Presentation),
                    game_authoring::DomainPresentation
                        .run_if(scene_residency::document_active)
                        .in_set(SceneEditorSystemSet::Presentation),
                ),
            )
            .add_systems(Update, orientation_axis_input
                .in_set(SceneEditorSystemSet::Input)
                .after(authoring::mesh_edit::MeshEditInput)
                .before(handle_placement_move)
                .before(control_editor_viewport)) // 先让元素状态机复核文档，再阻断观察控件释放帧的普通世界输入。
            .add_systems(
                Startup,
                (
                    ui::layout::restore_editor_layout_preferences,
                    spawn_scene_editor_scene,
                    load_scene_folder_asset,
                    request_initial_asset_scan,
                    setup_editor_preview_assets,
                    configure_scene_editor_brush_gizmos,
                    viewport::snapping::configure_grid,
                ),
            )
            .add_systems(
                Update,
                (
                    ui::scene_loading::suppress_scene_loading_shortcuts, // 切换场景只清除作者键盘边沿，不阻塞真实时间与后台任务。
                    ui::scene_loading_recovery::handle_recovery_buttons,
                    (update_scene_browser_drag, navigate_scene_browser_drag).chain(), // 先收口手势与落点，再允许悬停展开和树内自动滚动。
                    ui::scene_switch_dialog::handle_scene_switch_input, // 取消同帧仍独占输入；保存续接不新增文件事务。
                    ui::scene_documents::handle_scene_document_buttons,
                    ui::scene_switch_dialog::scroll_scene_switch_body, // 矮窗口只滚动说明，操作按钮始终可见。
                    ui::editor_settings::handle_settings_input,
                    (ui::editor_settings::handle_snap_inputs, ui::editor_settings::handle_mesh_lod_inputs).chain(),
                    ui::editor_settings::scroll_settings_body, // 偏好正文与吸附数值框复用新版滚动布局。
                    ui::shortcut_settings::scroll_shortcut_settings,
                    handle_scene_folder_dialog_input, // 模态关闭后把本帧所有权留给归类域，阻止底层补触发。
                    handle_scene_browser_input,       // 单击选择、双击打开、导航和结构按钮最后读取本帧所有权。
                    handle_terrain_conflict_buttons, // 模态选择不受普通作者输入run_if阻止。
                    (authoring::placement_batch::handle_placement_batch, authoring::placement_batch::sync_batch_selection).chain(),
                    handle_placement_move, // 独占状态机不能被自己的输入门暂停，否则永远收不到松手。
                    viewport::multi_select::handle_multi_select_pointer,
                )
                    .chain()
                    .before(update_scene_editor_numeric_scrub_mouse_guard) // 其他作者输入读取归类鼠标门前先完成本帧状态机。
                    .in_set(SceneEditorSystemSet::Input)
                    .in_set(GamePauseSystemSet::CollectBlockers),
            )
            .add_systems(
                Update,
                ui::water_layers::observe_water_creation_cancellation
                    .after(SceneEditorSystemSet::Input)
                    .before(SceneDocumentTransitionSet), // 目录模态、数值拖动及文档冻结都不能让已取消的静水请求恢复。
            )
            .add_systems(
                Update,
                (
                    ui::layout::handle_editor_layout_input.run_if(scene_browser_general_input_available),
                    ui::layout::remember_editor_layout_preferences, // 输入收口后仅发布显式选择和已结束的拖动。
                    update_scene_editor_numeric_scrub_mouse_guard,
                )
                    .chain()
                    .after(handle_placement_move)
                    .before(toggle_scene_editor)
                    .in_set(SceneEditorSystemSet::Input)
                    .in_set(GamePauseSystemSet::CollectBlockers),
            ) // 公共门始终推进，包括手柄 release 和静默帧；不能放入被门阻止的作者组。
            .add_systems(
                Update,
                (
                    (
                        (
                            (
                                toggle_scene_editor,
                                handle_editor_sidebar_input, // 页签和帮助开关先消费本帧点击，稳定工具不会抢回手动页面。
                                handle_editor_action_buttons,
                                viewport::snapping::handle_buttons,
                                handle_scene_base_terrain_row, // 点击右侧场景页的当前基础地形后展开网格设置。
                                handle_terrain_shading_buttons, // 平滑/平直只作用于已选地形，不进入作者撤销与保存。
                                handle_terrain_grid_button, // 显示网格独立切换，不改变平滑/平直状态。
                                handle_water_level_input, // 普通按钮可能切层；提交仍按聚焦时冻结的完整作者身份应用。
                                apply_water_level_scrub, // 左拖Changed实时刷新，Scrub松开只合并一条水位撤销。
                                apply_water_level_requests, // 保存、删除层或切换场景前先登记水位撤销与实时预览修订。
                                handle_scene_deletion_buttons,
                                handle_brush_numeric_inputs,
                            )
                                .chain(),
                            (
                                handle_vegetation_numeric_input, // 中键精确输入和右拖倍率按稳定层/组身份收口。
                                apply_vegetation_numeric_scrub, // 左拖只改轻量作者参数，松手一次撤销并启动一次编译。
                                handle_vegetation_authoring_buttons,
                                handle_vegetation_layer_row_clicks,
                                handle_water_layer_buttons,
                                apply_water_network_actions, // 水网按钮在静水层结构操作后读取最终连接候选。
                                handle_water_network_numeric_inputs, // 四项控制点参数按稳定ID收口精确输入与单次撤销拖动。
                                handle_water_feature_page_buttons, // 显式翻页直选目标页首个稳定水域对象。
                                handle_water_feature_row_clicks, // 数值与水网动作收口后再切当前静水、河流或瀑布。
                                apply_boundary_actions,
                                handle_boundary_thickness_input, // 墙厚Gizmo实时变化，松手才发布一次碰撞修订。
                                apply_environment_action,
                                handle_environment_numeric_inputs, // 四项天空亮度与Yaw复用同一公共交互协议。
                                handle_celestial_numeric_inputs, // 全局/场景天体值实时预览，提交边沿只登记一条撤销。
                                handle_celestial_override_toggles, // 启用覆盖复制当前有效值，恢复跟随删除对应字段。
                                handle_celestial_section_toggles,  // 两个纵向区域只改变会话布局。
                                (
                                    handle_world_environment_controls, // 时间、月份与运行时天气调试在推进/解析前生效。
                                    handle_grass_wind_slider, // 只写晴天基准设置，天气解析后由场景唯一风桥同帧派生实际摆幅。
                                )
                                    .chain(), // 局部嵌套避免超过外层系统元组上限，仍保持原有输入顺序。
                                handle_new_scene_form_input, // 尺寸公共提交已在前组完成，本系统只处理名称与Enter顺序。
                                handle_editor_shortcuts,
                                scroll_editor_side_panels,
                            )
                                .chain(),
                        )
                            .chain(), // 分组保持作者域严格顺序，同时避开超长系统元组实现上限。
                        (
                            handle_placement_transform_numeric_input, // 完整资产七字段先收口字符、拖动与灵敏度。
                            handle_placement_water_numeric_input, // 中键精确输入与右拖灵敏度先按旧Scene0选择收口。
                            apply_placement_water_numeric_scrub, // 左拖实时值在松开时只写一条有界PlacementChanged。
                            handle_asset_row_clicks,
                            handle_gltf_browser_input, // 独立行与分页组件不扩大全局EditorAction。
                            handle_placement_transform_actions, // 使用本帧最终完整实例选择执行移动、复制或删除。
                            handle_placement_water_actions, // 使用本帧最终场景树选择修改完整摆放记录。
                            control_editor_viewport,
                            handoff_gpu_blade_view,
                        )
                            .chain(),
                    )
                        .chain(),
                    (
                        handle_portal_list_row_clicks, // 当前行用于检查源点，目标行显式选择新点落点。
                        handle_portal_name_editor, // 当前源点名称支持中文输入、Enter应用和Esc取消。
                        handle_portal_delete_button, // 删除只作用于当前列表高亮源点，并登记可撤销作者命令。
                        apply_sparse_terrain_undo_redo, // 稀疏块差分先异步预取并恢复，旧历史系统不接触其作者源。
                        apply_undo_redo,
                        reconcile_portal_selection, // 撤销、切换场景或热更后同帧清理悬空稳定ID。
                        follow_editor_tool_with_sidebar, // 全部作者动作确定最终工具后，只在真实变化时定位相关页。
                        freeze_sidebar_tab_for_new_scene_dialog, // 弹框开闭时冻结/恢复左栏模块，取消或保存不得跳到地形。
                        sync_scene_editor_blocker,
                    )
                        .chain(),
                )
                    .chain()
                    .after(update_scene_editor_numeric_scrub_mouse_guard)
                    .run_if(scene_browser_general_input_available) // 文件夹模态、活动拖拽和release帧阻止全部其他作者输入。
                    .in_set(SceneEditorSystemSet::Input)
                    .in_set(GamePauseSystemSet::CollectBlockers),
            )
            .add_systems(
                Update,
                (
                    setup_editor_viewport,
                    setup_editor_orientation_axis,
                    setup_selected_water_badge, // 中央视口左上角只创建一次当前水域对象身份徽标。
                    setup_selected_portal_badge, // 传送工具复用第一信息位显示当前源点名称和坐标。
                    (setup_selected_placement_badge, ui::editor_settings::setup_snap_inputs, ui::editor_settings::setup_mesh_lod_inputs, viewport::multi_select::setup_multi_selection_ui).chain(), // 固定槽初始化不扩大外层系统元组。
                    sync_editor_viewport_render_size,
                    (
                        frame_selected_scene,
                        viewport::content_framing::frame_content,
                        viewport::framing::apply_viewport_framing,
                    )
                        .chain(),
                    sync_portal_directions_from_boundary, // 边界或传送修订变化时更新未保存预览的自动箭头方向。
                    resolve_selected_placement_mesh, // 选择或World实例换代后按固定预算解析精确primitive。
                    sync_selected_water_viewport, // 相机应用前解析精确水面、同步高亮并消费双击定位。
                    sync_selected_portal_viewport, // 当前源点单击只标记，双击保持视图类型并定位完整光幕。
                    sync_selected_placement_viewport, // 复用活动Scene0 Mesh并在双击后调整当前视角。
                    sync_editor_viewport_camera, // 必须使用本帧刚采用的草稿、最终视口尺寸和场景取景。
                    (resolve_vegetation_ground_cursor.after(feathrise_scene::ClipmapGroundPublishSet),
                        viewport::water_plane::sync_water_plane_cursor,
                        viewport::source_fence::update).chain(), // 真实曲面采用后先校验只读来源范围，再放行各作者工具。
                    prefetch_sparse_terrain_cursor, // 未驻留非平地页在后台预取，后续帧重新做真实高度射线命中。
                    (viewport::modes::block_overlay_cursor.in_set(game_authoring::BaseGroundCursorResolved), viewport::snapping::update_mesh_cursor).chain(), // 先隔离浮层，再合并普通地形的吸附补查。
                    ui::courtyard::viewport_input
                        .run_if(scene_editor_numeric_scrub_mouse_available)
                        .run_if(scene_browser_general_input_available), // 院落框选与原视口入口共用模态和拖动鼠标门。
                    select_editor_viewport_content
                        .run_if(scene_editor_numeric_scrub_mouse_available)
                        .run_if(scene_browser_general_input_available), // 相机发布真实射线后，查看工具只在左键边沿解析对象或植被整层。
                    sync_editor_orientation_axis, // 方向轴读取同帧最终相机姿态，不创建第二个三维渲染View。
                    sync_scene_editor_custom_cursor.in_set(game_authoring::EditorCursorPresentation), // 应用落点采用后才隐藏系统鼠标，且保持CursorGrabMode::None。
                )
                    .chain()
                    .run_if(scene_residency::document_active) // 分帧恢复时不能提前打开目标相机或用旧作者源改写目标场景。
                    .in_set(SceneEditorSystemSet::Cursor),
            )
            .add_systems(
                Update,
                (
                    (
                        apply_scene_folder_asset_events,
                        poll_scene_folder_save,
                        poll_scene_folder_batch, // 完整批量结果先核对身份，再串行保存和一次采用。
                        poll_scene_edit, // 场景名称与备注事务使用稳定身份及共享写入互斥。
                        advance_scene_browser_index,
                        start_asset_scan,
                        poll_asset_scan,
                        poll_scene_creation,
                        poll_scene_deletion,
                        sync_global_environment_authoring, // AssetServer 全局配置首次加载或热更后更新未脏作者基线。
                        poll_sparse_terrain_task, // 完成值按场景和票据原子归还唯一作者会话。
                        sync_sparse_terrain_authoring, // 新SparseClipmap草稿只载入清单，不枚举所有块。
                        reconcile_following_placements, // 草稿或外部GLB采用完成后每帧最多更新128条跟随型摆放。
                        refresh_gltf_browser_cache, // 草稿采用后同帧同步强Scene0句柄；关闭F12则释放强引用。
                        ui::scene_documents::restore_scene_document_selection,
                        poll_vegetation_blank_layer_allocation, // 身份匹配的完整空白R8才加入正式输出、作者层和撤销栈。
                        poll_vegetation_compile,
                    )
                        .chain(), // 前半组完成目录、草稿与浏览缓存采用，保持原有严格顺序。
                    (
                        (
                            poll_water_file_handoff, // 先消费已有JSON/R8/GLB地形事件；随后本帧新建的保存门只会看到发布后的新消息。
                            cleanup_placement_handoff_previews, // 严格新Ready同帧只回收本批精确Scene0实体。
                            cleanup_saved_portal_previews, // 传送正式新修订完整Ready后才无缝回收作者预览。
                            start_global_environment_save, // 仅全局脏时直接消费统一保存请求并绕过场景预检。
                            poll_global_environment_save, // 单文件结果非阻塞采用并立即发布动态场景配置。
                            start_sparse_terrain_save, // 新图先原子提交脏块与清单，再继续其余作者文件。
                            start_draft_save,
                            poll_draft_save,
                            ui::scene_switch_dialog::resume_scene_switch_after_save, // 只有完整保存成功回执才继续原来冻结的场景目标。
                        )
                            .chain(),
                        (
                            poll_surface_preview_rebuild,
                            poll_density_overlay_rebuild,
                            poll_vegetation_preview_rebuild,
                            poll_water_preview_rebuild,
                        )
                            .chain(),
                    )
                        .chain(), // 后半组完成保存与各类后台预览轮询，避免超过Bevy系统元组实现上限。
                )
                    .chain()
                    .in_set(SceneEditorSystemSet::Tasks),
            )
            .add_systems(
                Update,
                (
                    (
                        (
                            spawn_dynamic_rows,
                            spawn_gltf_browser_rows, // BSN容器首次出现时建立固定八行池。
                            setup_water_level_input, // BSN槽位只挂载一次公共NumericInput根。
                            setup_water_network_numeric_inputs, // 四个控制点槽位各挂载一次完整公共专业字段。
                            setup_boundary_thickness_input, // 区域边界旧加减按钮替换为一项完整米制字段。
                            setup_environment_numeric_inputs, // 右栏四个天空环境槽位挂载开箱即用公共字段。
                            setup_celestial_numeric_inputs, // 全局与场景二十个天体标量挂载精确输入及拖动控件。
                            setup_world_environment_controls, // 原 F1 世界环境组在 F12 固定槽位只生成一次。
                            setup_world_environment_slider_layout, // 五条原轨道仅在创建帧适配栏宽，不改变运行时调试映射。
                            setup_vegetation_numeric_inputs, // 五个植被作者参数挂载完整公共专业字段。
                            spawn_vegetation_layer_rows,
                            spawn_water_feature_rows, // 水域面板只建立一次六行固定对象池。
                            spawn_portal_list_rows.after(ui::scene_inspector::sync_selected_tool_properties), // 先移挂所选传送表单，再建立本帧列表。
                        )
                            .chain(),
                        (
                            configure_brush_slider_focus,
                            configure_delete_scene_dialog_focus,
                            configure_terrain_conflict_dialog_focus,
                            edit_boundary_from_viewport, // 始终运行输入门，阻塞帧取消旧点击并收口已受理拖点。
                            edit_water_network_from_viewport
                                .run_if(scene_editor_numeric_scrub_mouse_available)
                                .run_if(scene_browser_general_input_available), // 顶视/透视逐点与XZ拖点读取本帧最终地形命中。
                            (draw_scene_editor_brush, viewport::snapping::draw_grid).chain(),
                            draw_scene_boundaries,
                            draw_water_network,
                            draw_selected_water_highlight_frame, // 精确淡青填充外围补充铜金世界范围。
                            draw_selected_portal_marker, // 当前源点以触发外环和高标杆在中央画面中明确定位。
                            draw_selected_placement_highlight_frame, // Scene0精确覆盖外围补充三维铜金包围框。
                            draw_placement_move, // 三轴与XZ方框随模型预览移动；建筑经原校验通过才写作者记录。
                            apply_sparse_terrain_brush
                                .run_if(scene_editor_numeric_scrub_mouse_available)
                                .run_if(scene_browser_general_input_available),
                            apply_surface_brush
                                .run_if(scene_editor_numeric_scrub_mouse_available)
                                .run_if(scene_browser_general_input_available),
                            draw_surface_stroke_trail, // 真实输入受理后同帧显示柔边填色，包括极短点击和后台待补。
                            sync_surface_selection_overlay, // 笔迹更新后决定选中面显隐，不叠加两种辅助色。
                            (apply_puddle_brush, sync_puddle_preview).chain(), // 后台笔刷完整采用后发布独立GPU遮罩。
                            apply_vegetation_brush
                                .run_if(scene_editor_numeric_scrub_mouse_available)
                                .run_if(scene_browser_general_input_available),
                            apply_water_brush
                                .run_if(scene_editor_numeric_scrub_mouse_available)
                                .run_if(scene_browser_general_input_available),
                        )
                            .chain(),
                    )
                        .chain(),
                    (
                        (
                            setup_placement_water_numeric_inputs, // 属性区两个固定槽位只挂载一次公共NumericInput根。
                            setup_placement_transform_numeric_inputs, // 完整实例七字段各挂载一个公共专业输入根。
                            start_vegetation_compile,
                            sync_live_blade_density,
                            sync_live_clipmap_water,
                            sync_density_overlay,
                            place_from_viewport
                                .run_if(ui::courtyard::mode_idle) // 院落落点不能被单体资产或玩法放置接管。
                                .run_if(scene_editor_numeric_scrub_mouse_available)
                                .run_if(scene_browser_general_input_available),
                            apply_selected_placement_relocation
                                .run_if(ui::courtyard::mode_idle) // 旧单体吸附目标不能抢同一院落点击。
                                .run_if(scene_editor_numeric_scrub_mouse_available)
                                .run_if(scene_browser_general_input_available), // 新资产放置让出后，同一左键只移动所选完整实例。
                            start_surface_preview_rebuild,
                            start_density_overlay_rebuild,
                            start_vegetation_preview_rebuild,
                            start_water_preview_rebuild,
                        )
                            .chain()
                            .run_if(scene_residency::document_active),
                        (
                            (
                                suppress_new_runtime_mesh_roots
                                    .run_if(scene_residency::document_active),
                                suppress_new_runtime_water_surfaces
                                    .run_if(scene_residency::document_active),
                                ui::editor_settings::sync_terrain_backfaces_preview,
                                finish_scene_editor_close.after(TerrainLiveSet::React),
                                sync_terrain_shading_session, // 关闭F12保留着色与网格，切换场景按真实玩法场景恢复本进程选择。
                                sync_editor_weather_preview.run_if(scene_residency::document_active), // 关闭门同帧归还自然天气；打开时选择F12唯一视觉锚点。
                                sync_dynamic_celestial_preview
                                    .run_if(scene_residency::document_active), // 全局草稿与场景覆盖同帧生成唯一动态场景光照预览。
                                sync_editor_skybox_requests.run_if(scene_residency::document_active),
                                sync_live_boundary_override.run_if(scene_residency::document_active),
                                sync_boundary_text,
                                sync_water_network_text,
                                sync_editor_layer_status,
                            )
                                .chain(),
                            (
                                (
                                    sync_water_level_input, // 切层、撤销、重做和草稿换代后回写精确水位。
                                    sync_water_network_numeric_inputs, // 选择、连接、撤销和倍率变化后同步四项控制点值。
                                    sync_boundary_thickness_input, // 路径换代与保存门同步墙厚值和禁用态。
                                    sync_environment_numeric_inputs, // 四项天空作者值同步对应公共输入根。
                                    sync_vegetation_numeric_inputs, // 切层、规则、锁定、撤销和倍率变化同步五项参数。
                                    refresh_placement_water_selection_cache, // 选择或作者修订变化时只定位一次当前水绑定。
                                    refresh_placement_water_runtime_selection_cache, // 生命周期票据变化时只聚合当前选择的少量根。
                                    sync_placement_water_numeric_inputs, // 选择、撤销和设置换代后同步值、禁用态与独立倍率。
                                    sync_placement_transform_panel, // 完整实例选择、撤销和设置换代后同步七字段与动作。
                                    sync_brush_sliders,
                                    sync_delete_scene_dialog,
                                    ui::scene_switch_dialog::sync_scene_switch_dialog,
                                    sync_terrain_conflict_dialog,
                                    sync_scene_folder_dialog,
                                )
                                    .chain(),
                                (
                                    sync_placement_water_runtime_status, // 先把真实Ready/Failed回执写入底栏，再统一刷新壳文本。
                                    sync_editor_sidebar_presentation, // 八页实体原位切换并回显活动页签与折叠说明。
                                    sync_editor_sidebar_context, // 水域、植被和网格属性只显示当前选择真正适用的分组。
                                    sync_portal_selection_panel, // 仅传送工具激活时让双列表参与玩法页布局。
                                    (
                                        sync_editor_shell,
                                        ui::world_session::sync_editor_world_controls,
                                        ui::scene_documents::sync_scene_documents,
                                        ui::scene_loading::sync_scene_loading,
                                        ui::scene_loading_recovery::sync_recovery_buttons,
                                        ui::layout::sync_editor_layout,
                                    )
                                        .chain(),
                                    (
                                        sync_new_scene_dialog_status,
                                        scroll_new_scene_dialog_body, // LOD内层未悬停时滚动资料正文，底部按钮不移动。
                                    )
                                        .chain(), // 动态LOD先更新内容尺寸，随后同帧钳制外层正文偏移。
                                    sync_dynamic_list_layouts, // 先同步场景树滚动高度，再钳制资产页码与列表高度。
                                    sync_scene_rows,
                                    sync_scene_browser_root_row, // 固定根目录行始终可选择并接收场景拖回根级。
                                    sync_scene_browser_drag_preview,
                                    sync_asset_rows,
                                    sync_gltf_browser_rows, // 缓存与分页完成后原地回显Scene0树。
                                    ui::gltf_browser::sync_scene_base_gltf_row, // GLB和Clipmap基础地形独立于摆放实例计数。
                                    (
                                        sync_placement_water_text, // 网格选择、撤销或草稿换代后回显实际水体语义。
                                        ui::scene_inspector::sync_scene_inspector, // 概览和反馈只读取同帧最终选择与真实状态。
                                    )
                                        .chain(),
                                    (
                                        sync_vegetation_preset_list, // 开发者目录热更时有界增删并重绑无分页三列列表。
                                        sync_vegetation_layer_rows,
                                        sync_vegetation_layer_row_interactions,
                                    )
                                        .chain(), // 预设列表、作者行内容和即时交互保持局部严格顺序。
                                    sync_water_feature_rows, // 专用水域修订、选择或页码变化时才刷新当前六行。
                                    sync_water_feature_row_interactions, // 行悬停、按下和稳定选中态共享水墨铜边反馈。
                                    sync_current_portal_rows, // 当前场景全部源点按传送修订或选择刷新。
                                    sync_portal_name_editor, // 所选源点名称、IME候选和聚焦边框原位回显。
                                    sync_target_portal_rows, // 目标场景列出默认出生点和具体arrival端点。
                                )
                                    .chain(),
                            )
                                .chain(),
                        )
                            .chain(),
                    )
                        .chain(),
                )
                    .chain()
                    .in_set(SceneEditorSystemSet::Presentation),
            )
            .add_systems(
                Update,
                (
                    sync_editor_action_button_styles,   // 静态工具按钮沿用统一按压反馈。
                    viewport::snapping::sync_buttons,
                    sync_terrain_shading_button_styles, // 场景页地形模式独立高亮，不影响其他作者按钮。
                    sync_terrain_grid_button_style, // 显示网格使用同一铜金交互态并允许与任一着色模式同时选中。
                    sync_brush_slider_interaction_styles, // 紧凑数值拖动区只在交互变化时强化暗墨铜边。
                    sync_gltf_browser_page_button_styles, // 专用分页按钮不进入全局动作样式查询。
                    sync_placement_water_button_styles
                        .after(refresh_placement_water_selection_cache), // 当前水体类别读取同帧完成的共享派生值。
                    sync_current_portal_row_interactions, // 当前源点行提供即时按下与悬停反馈。
                    sync_target_portal_row_interactions,  // 目标落点行保持独立选择反馈。
                )
                    .in_set(SceneEditorSystemSet::Presentation),
            )
            .add_systems(
                Update,
                (
                    sync_celestial_controls, // 数值、继承按钮和颜色色块读取本帧最终作者与运行时采用结果。
                    sync_editor_environment_text, // 天空与全局配置诊断在最终光照之后刷新。
                    sync_world_environment_controls, // 时间、月份和天气摘要等待最终效果应用后刷新。
                )
                    .chain()
                    .after(SceneEditorSystemSet::Presentation)
                    .after(WorldTimeSystemSet::ApplyLighting)
                    .after(WeatherSystemSet::ApplyEffects),
            )
            .add_systems(
                Update,
                ui::ime::sync_scene_editor_ime.after(SceneEditorSystemSet::Presentation), // 所有点击、切页和模态状态收口后，只写一次主窗口IME最终值。
            )
            .add_systems(
                Update,
                sync_editor_camera_activity.after(SceneEditorSystemSet::Presentation), // 关闭门之后仍执行，作者恢复/切换场景不能遗留活动编辑相机。
            )
            .add_systems(
                PostUpdate,
                (ui::editor_settings::sync_settings_panel, ui::editor_settings::sync_snap_inputs, ui::editor_settings::sync_mesh_lod_inputs)
                    .before(bevy::ui::UiSystems::Prepare),
            )
            .add_systems(
                PostUpdate,
                ui::shortcut_settings::sync_shortcut_settings_panel
                    .before(bevy::ui::UiSystems::Prepare),
            )
            .add_systems(
                PostUpdate,
                position_scene_browser_drag_preview
                    .after(bevy::ui::UiSystems::PostLayout), // 中文完成换行后再贴近鼠标，避免首帧高度及缩放误差。
            )
            .add_systems(
                PostUpdate,
                ui::sidebars::sync_asset_reveal_scroll
                    .after(bevy::ui::UiSystems::PostLayout) // 固定行池已经绑定新页并完成布局后才滚动。
                    .after(bevy::transform::TransformSystems::Propagate), // 读取与实际命中相同的最终UI坐标。
            )
            .add_systems(
                PostUpdate,
                sync_editor_portal_labels
                    .before(bevy::transform::TransformSystems::Propagate) // 最终F12朝向必须在GlobalTransform传播前写入。
                    .before(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate), // 本帧渲染提取读取最终标签显隐。
            )
            .add_systems(
                OnExit(ScenePhase::Active),
                (
                    clear_vegetation_previews,
                    clear_water_previews,
                ),
            );
        ui::window_camera::install(app); // 外壳的窗口尺寸及DPI不随驻留场景默认相机切换。
        ui::tooltips::install(app); // 单一提示浮层只读当前按钮状态，不改变布局、鼠标所有权或作者事务。
        ui::status_bar::install(app); // 底栏分区与完整诊断只消费真实回执，展开不改变中央视口尺寸。
        ui::context_help::install(app); // 操作说明只在唯一帮助浮层展开，不挤占作者面板或点透场景。
        ui::vegetation_preset_editor::install(app); // 预设全局写入与GLTF检查按编辑器既有Input/Tasks/Presentation阶段接入。
        authoring::boundary::objects::install(app); // 空气墙独立对象管理。
        // 原游戏的角色、装备和技能作者域由游戏应用安装，不混入普通场景作者插件。
        ui::surface_preset_editor::install(app); // PBR地表目录与内容寻址数组共用中央多标签工作区。
        ui::surface_layers::install(app); // 当前场景地表层以固定列表选择，整图删除在后台原子收口并支持撤销。
        ui::terrain_sidebar::install(app); // 工具页只发布原工具请求，选择模式继续禁止落笔。
        ui::scene_sidebar::install(app); // 浏览身份和真实操作反馈只读显示，不建立第二份场景或目录。
        ui::subscenes::install(app); // 完整子场景只拥有引用和整体变换，内部内容仍走源场景文档。
        ui::terrain_objects::install(app);
        authoring::terrain_ground::install(app);
        authoring::terrain_editing::install(app);
        authoring::terrain_documents::install(app);
        authoring::vegetation_paint_scope::install(app);
        authoring::model_vegetation::install(app);
        ui::vegetation_paint::install(app);
        authoring::mesh_lod::install(app);
        viewport::subscenes::install(app);
        viewport::terrain_objects::install(app);
        ui::new_scene_dialog::install(app); // 资料优先、规格只读摘要和会话折叠不接管原输入或场景事务。
        ui::management_dialogs::install(app); // 删除、目录及外部地形冲突只统一表现和独立滚轮。
        authoring::install_surface_preview_render(app); // 未保存地表使用正式PBR并等待编辑视口GPU管线就绪。
        ui::water_preset_editor::install(app); // 静水预设沿用共享中央标签区，目录与贴图继续由AssetServer热采用。
        ui::water_sidebar::install(app); // 工具准备走原模式门；作者列表与事务保持同一来源。
        ui::vegetation_sidebar::install(app); // 仅观察当前身份的画布、编译及密度发布，不新增作者事务。
        ui::gameplay_sidebar::install(app); // 工具页不自动进入编辑；三类玩法对象继续沿用原作者协议。
        ui::building_presets::install(app); // 建筑目录与专用点放复用中央标签和后台原子保存。
        ui::courtyard::install(app); // 院落 UI、多选和输入请求使用固定预算扫描与场景身份门。
        ui::building_sidebar::install(app); // 工具页切换先收口旧输入，状态与参数仍由原领域回显。
        authoring::buildings::install(app); // 建筑placement使用有界索引、后台规则校验与专用Gizmo落点。
        authoring::courtyards::install(app); // 院落选集在公共目录热采用后以整组历史和后台碰撞校验落地。
        ui::viewport_toolbar::install(app); // 地图与 GLB 的同一工具栏展开规则。
        ui::asset_preview::install(app); // 右侧所选图片与Scene0使用独立、按需启停的预览视图。
        ui::asset_thumbnails::install(app); // 固定行池切换网格，一个相机按可见项串行生成静态快照。
        ui::external_files::install(app); // 路径检查与外部进程启动不占编辑器帧关键路径。
        ui::asset_context_menu::install(app); // 复制、外部编辑和文件夹入口共用鼠标门。
        ui::scene_context_menu::install(app); // 对象菜单与中文重命名复用原作者事务及静默释放门。
        ui::scene_layers::install(app); // 四组固定行池展示场景里已有的水域、植被与角色。
        ui::asset_shortcuts::install(app); // 收藏/最近访问复用目录定位，个人记录在后台原子保存。
        editor_monitor::install_editor_scene_monitor(app); // 独立F4面板读取上方已初始化作者与预览资源，不改变F3职责。
        app.add_systems(Last, ui::scene_loading_recovery::advance_recovery);
        app.add_systems(
            Update,
            (
                ui::scene_documents::poll_scene_document_history,
                drive_pending_scene_transition,
                ui::scene_documents::finish_scene_document_close,
                ui::scene_documents::preserve_document_on_external_scene_change,
                start_draft_load,
                poll_draft_load,
            )
                .chain()
                .in_set(SceneDocumentTransitionSet),
        ); // 场景暂停期间仍非阻塞推进交接及显式关闭。
        app.add_systems(
            Update,
            layer_candidate_interactions
                .after(feathrise_editor_ui::EditorUiSystemSet::NumericInput)
                .before(SceneEditorSystemSet::Input),
        ) // 浮层先拥有点击，禁止输入穿透场景。
        .add_systems(
            Update,
            sync_sidebar_layer_selection
                .after(SceneEditorSystemSet::Input)
                .before(viewport::modes::sync_mode),
        ) // 先捕获侧栏本帧选层，再由模式门收回Inspect；旧候选不能覆盖新侧栏选择。
        .add_systems(
            Update,
                (poll_layer_candidates, apply_layer_candidate, viewport::multi_select::advance_multi_selection,
                    viewport::multi_select::sync_multi_selection_ownership,
                    ui::scene_inspector::sync_selected_tool_properties) // 正式选择采用后移挂唯一表单，Presentation读取本帧最终宿主。
                .chain()
                .in_set(SceneEditorSystemSet::Tasks),
        ) // 完整候选结果在命中和高亮之前原子采用。
        .add_systems(
            Update,
            (sync_layer_candidate_ui, viewport::multi_select::sync_multi_selection_ui, viewport::multi_select::draw_multi_selection)
                .in_set(SceneEditorSystemSet::Presentation),
        ); // 候选展开与翻页只影响固定预算UI。
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test] // 完整编辑器调度验证官方输入提前应用和最终焦点收口没有循环或Query冲突。
    fn official_editor_input_schedule_initializes_without_conflicts() {
        let mut app = App::new(); // 不启动窗口或渲染，只构造真实插件系统图。
        app.add_plugins((MinimalPlugins, AssetPlugin::default())); // 正式编辑器安装会登记资产类型，夹具需要资产服务和任务池。
        app.add_plugins((SceneEditorPlugin, feathrise_editor_glb::GlbEditorPlugin));
        let mut schedule = app
            .world_mut()
            .resource_mut::<bevy::ecs::schedule::Schedules>()
            .remove(Update)
            .unwrap(); // 取出真实Update图进行初始化。
        schedule
            .initialize(app.world_mut())
            .expect("官方输入与作者业务调度必须无环且查询互斥"); // 同时初始化系统参数并检查顺序约束。
    } // 不执行任何场景文件或后台作者任务。

    #[derive(Resource, Default)]
    struct PlacementSaveContractProbe(bool);

    #[test] // 使用真正场景与编辑器插件，验证新增地表发布先后关系没有跨插件环。
    fn clipmap_ground_scene_editor_schedules_initialize_together() {
        // 无窗口验证Update与覆盖所在PostUpdate。
        use bevy::ecs::schedule::ScheduleLabel; // 将不同具体调度标签统一为驻留标识。
        let mut app = App::new(); // 不执行场景加载或作者写入。
        app.add_plugins((MinimalPlugins, AssetPlugin::default())); // 提供真实任务池及资产服务。
        app.init_asset::<Shader>()
            .init_asset::<Image>()
            .init_asset::<Mesh>(); // 场景内置材质注册需要这些宿主资产。
        app.add_plugins((feathrise_scene::GameScenePlugin, SceneEditorPlugin, feathrise_editor_glb::GlbEditorPlugin)); // 验证生产系统的顺序关系。
        for label in [First.intern(), Update.intern(), PostUpdate.intern()] {
            // 同时覆盖光标采用和贴地覆盖显示。
            let mut schedule = app
                .world_mut()
                .resource_mut::<bevy::ecs::schedule::Schedules>()
                .remove(label)
                .unwrap(); // 取出对应实际调度图。
            schedule
                .initialize(app.world_mut())
                .expect("Clipmap地表发布、编辑器输入与覆盖显示必须无环且查询互斥"); // 初始化验证借用冲突和跨插件约束。
        } // 不启动游戏、不创建窗口、不运行磁盘任务。
    } // 现有GLB系统同时参与调度回归。

    fn exercise_placement_save_contracts(
        _status: Res<ScenePlacementRuntimeStatus>,
        mut requests: MessageWriter<ScenePlacementRebuildRequest>,
        mut probe: ResMut<PlacementSaveContractProbe>,
    ) {
        requests.write(ScenePlacementRebuildRequest {
            scene_id: feathrise_scene_content::SceneId::parse("MAP-editor-plugin-contract")
                .expect("测试场景ID合法"),
        });
        probe.0 = true; // Update实际构造两个SystemParam，不能只检查World里恰好存在资源。
    }

    #[test]
    fn standalone_editor_registers_placement_save_resources_and_messages_idempotently() {
        let mut app = App::new();
        init_placement_save_runtime_contracts(&mut app);
        init_placement_save_runtime_contracts(&mut app); // 模拟scene插件也注册同一公开契约。
        app.init_resource::<PlacementSaveContractProbe>()
            .add_systems(Update, exercise_placement_save_contracts);

        app.update();

        assert!(app.world().resource::<PlacementSaveContractProbe>().0); // 独立首个Update不会因缺资源或消息队列失败。
    }
}
