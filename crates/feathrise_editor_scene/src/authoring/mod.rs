//! 场景作者文件创建、原子发布和派生白模GLB构建。

pub(crate) mod boundary; // 区域空气墙重画、控制点编辑、可视预览和未保存物理覆盖。
pub(crate) mod basic_shapes; // 内置基础几何体添加、点放预览与显式空场景工作平面。
pub(crate) mod mesh_edit; // 独立网格编辑模式：共享顶点、边面选择、局部移动和完整记录历史。
pub(crate) mod buildings; // 建筑实例分帧空间索引、后台落点校验和专用Gizmo预览。
mod celestial; // 全局天体作者基线与动态场景未保存实时预览。
mod celestial_input; // 全局基础值、场景逐属性覆盖与继承切换的专业数值输入。
pub mod domain_documents; // 游戏正文只通过不透明版本与既有事务端口接入。
pub mod retirement;
pub use retirement::AuthoringRetirement;
pub(crate) use retirement::poll as poll_authoring_retirement;
mod commands; // 可撤销局部差分与作者图层结构快照普通类型。
#[cfg(test)]
pub(crate) use commands::ByteChange; // 有限窗口夹具的来源回归复用正式差分类型，不扩大生产接口。
pub(crate) mod courtyards; // 院落模板保存、分帧成员索引和整组受检作者事务。
mod create; // 完整新场景事务：先构建私有目录，最后发布全局目录入口。
pub(crate) mod creation_input; // 名称、米制范围与有界高度场规格校验供弹框预览复用。
mod delete; // 受保护场景校验、传送引用检查与可恢复后台删除事务。
mod density_overlay; // 当前植被层以局部即时、全层后台方式覆盖到地形网格。
mod edit; // 显式模型与传送点放置、目标选择和撤销重做。
mod environment; // 场景天空作者修改和F12正式天气预览同步，场景气候只读。
mod environment_input; // 天空亮度、方向与天气过渡复用公共专业数值框。
mod files; // 受限于assets/scenes的原子文件替换工具。
mod sculpt_paths;
pub(crate) use sculpt_paths::{SculptPathLibrary, load_sculpt_paths, save_sculpt_path};
mod geometry; // GLB与实时预览共用的规则高度场几何构建器。
mod glb; // 不依赖Blender的单primitive白模地形GLB编码器。
mod image_io; // 旧场景PNG密度与高度单元在IO边界归一成统一作者草稿。
pub(crate) use image_io::encode_density_png; // 内存工程快照与正式保存共用唯一PNG编码，不另造数值纹理格式。
mod ktx2; // Rust直接生成地表、密度和高度单元的未压缩KTX2派生纹理。
mod live_density; // 活动Blade层局部写入正式Image，复用现有GPU四级窗口即时显示。
mod live_density_dynamic; // 新建动态Blade层以后台内存快照进入正式运行时。
mod live_water_dynamic; // Clipmap完整水网以同修订定义、R8和GPU图片发布给静水环带与Ribbon后端。
mod load; // 当前场景JSON、动态terrain.glb与兼容高度资产的有界后台加载。
pub(crate) mod puddles; // 独立积水层后台笔刷、完整预览与同事务保存。
pub(crate) use puddles::{PuddleAuthoringState, apply_puddle_brush, sync_puddle_preview}; // 插件统一编排积水生命周期。
mod paint;
pub use paint::brush_path_dab_count; // 作者域复用同一圆刷轨迹间距，不公开其余场景写侧。 // 地表RGBA与植被R8局部笔刷。
mod placement_grounding; // 显式摆放支持固定高度或按离地偏移分帧跟随R16地形。
pub(crate) mod content_bindings; // 地形关联更新传送点与外部作者文档，固定世界内容不变。
pub(crate) mod terrain_ground; // 多地形共享真实表面索引，空白处不提供隐式承托面。
pub(crate) mod terrain_editing; // 所选独立地形的源版本编辑；保存和历史仅交换受检版本路径。
pub(crate) mod terrain_documents; // 对象画布槽、来源投影和统一历史身份。
pub(crate) mod mesh_lod; // 作者模式临时请求完整几何，保留运行时LOD偏好与原资产身份。
pub(crate) mod placement_batch; // 跨场景剪贴板、受检粘贴预览和单步批量复制/删除。
pub(crate) mod placement_pose; // 摆放记录的所属源框架与精确世界姿态换算，供选择、变换及复制共用。
pub(crate) mod placement_preview_ownership; // 独立作者预览定向遮蔽正式候选，精确保存或冷载后归还。
pub(crate) mod placement_deletion; // 文档持有删除票据，约束旧保存迟到Add及精确撤销/保存确认。
mod portal_direction; // 传送箭头按当前主区域边界最近线段点自动派生。
#[path = "preview/mod.rs"]
mod preview; // 地表综合色与正式多档Mesh植被按职责拆分的有界后台预览。
pub(crate) use preview::{same_surface_canvas, surface_canvas_mesh}; // 覆盖与材质预热共用当前对象的固定成本来源。
mod resources; // 草稿、撤销历史、预览资产和任务槽。
#[cfg(test)]
pub(crate) use resources::TerrainPreviewIdentity; // 真实对象覆盖回归冻结已采用地形票据，不开放私有资源模块。
pub(crate) mod volume_sculpt; // 局部体积原型只通过密度后台、联合场景发布和场景原子保存进入作者流程。
mod save; // JSON、动态terrain.glb和兼容派生资产的后台原子保存。
pub(crate) mod sparse_terrain; // 新图局部高度笔刷、差分与后台IO可调用的稀疏作者会话。
mod surface_layer; // 点击预设立即创建作者层，笔刷复用相同去重与撤销事务。
pub(crate) use surface_layer::create_surface_layer; // UI显式新建独立作者层。
mod surface_layer_edit; // 地表作者层删除、四槽重映射和独立写侧single-flight事务。
mod terrain; // 实际GLB拓扑笔刷、撤销差分和后台完整地形预览。
mod terrain_external; // Blender GLB自动采用、脏草稿冲突与一次性覆盖授权。
mod terrain_live; // 未保存GLB顶点草稿直接接管正式动态地形画面与Avian Trimesh。
mod vegetation_compile; // 多张作者R8按组规则后台编译为原有固定正式运行时层。
pub(crate) use vegetation_compile::author_layers_at_world; // 统一视口后台候选只复用纯采样接口。
mod vegetation_defaults; // 新场景和旧草稿迁移共用六个正式密度层定义。
mod vegetation_layers; // 作者层选择到正式profile、所属组和R8画布的唯一映射入口。
mod vegetation_load; // 正式输出与独立作者R8在IO任务中按兼容规则完整装载。
mod vegetation_paint; // 独立作者R8笔刷、写时复制和撤销差分收口。
pub(crate) mod vegetation_paint_scope; // 场景级绘制组与跨网格整笔事务。
pub(crate) mod water_canvas; // 静水独立源范围、姿态与写保护，不要求地形网格或高度页。
mod water_defaults; // 新场景和旧动态草稿迁移共用单层河水R8画布。
mod water_handoff; // 保存后等待R8事件与正式动态水新代次原子Ready的关闭门。
mod water_level; // 静止水位精确提交、实时左拖和单次撤销事务。
mod water_load; // 未登记Clipmap水域只在后台读取标准私有文件并冻结并发基线。
mod water_network; // 连续河流样条、急流段和瀑布水幕作者工具。
mod water_paint; // 河水R8覆盖与GLB地形顶点河床复合笔刷。
mod water_preview; // R8遮罩后台建网格并安装正式GPU波浪材质预览。

pub(crate) use boundary::{
    BoundaryThicknessInputSlot, BoundaryThicknessInputState, apply_boundary_actions,
    draw_scene_boundaries, edit_boundary_from_viewport, handle_boundary_thickness_input,
    setup_boundary_thickness_input, sync_boundary_text, sync_boundary_thickness_input,
    sync_live_boundary_override,
};
pub(crate) use celestial::{sync_dynamic_celestial_preview, sync_global_environment_authoring};
pub(crate) use celestial_input::{
    CelestialColorSwatch, CelestialNumericInputSlot, CelestialNumericInputState,
    CelestialNumericKey, CelestialNumericScope, CelestialOverrideProperty, CelestialOverrideToggle,
    CelestialOverrideToggleLabel, CelestialSectionBody, CelestialSectionState,
    CelestialSectionToggle, CelestialSectionToggleLabel, CelestialValueField,
    handle_celestial_numeric_inputs, handle_celestial_override_toggles,
    handle_celestial_section_toggles, setup_celestial_numeric_inputs, sync_celestial_controls,
};
pub use commands::{
    BuildingPlacementChange, BuildingTerrainChange, CourtyardBatchChange, EditorCommand,
    SceneObjectChange, SceneObjectBatchChange,
    HeightChange, VegetationAuthoringChange, WaterLayersChange, command_uses_bounded_water_history,
    placement_structure_differs, placement_water_differs,
};
#[cfg(test)]
pub(crate) use commands::push_bounded_water_history; // 仅纯记录回归保留旧Vec接口；生产统一走会话入口。
pub(crate) use create::{poll_scene_creation, request_scene_creation};
pub(crate) use delete::{scene_deletion_request, poll_scene_deletion};
pub(crate) use density_overlay::{
    VegetationDensityOverlayState, poll_density_overlay_rebuild, start_density_overlay_rebuild,
    sync_density_overlay,
};
pub(crate) use edit::{
    apply_undo_redo, next_author_id, place_from_viewport, refresh_owned_portal_preview,
    setup_editor_preview_assets, spawn_placement_preview, placement_record_bounds, attach_patch_preview, patch_preview_parent,
}; // 传送目标改由UI双列表显式选择，名称修改可刷新未保存预览。
#[cfg(test)]
pub(crate) use edit::drive_undo_redo_to_completion;
#[cfg(any(test, feature = "validation"))]
pub(crate) use environment::commit_environment_change;
pub(crate) use environment::{
    apply_environment_action, sync_editor_skybox_requests, sync_editor_weather_preview,
}; // 同步天空请求和不持久化的F12天气画面覆盖，不修改场景气候身份。
pub(crate) use environment_input::{
    EnvironmentNumericInputSlot, EnvironmentNumericInputState, EnvironmentNumericKind,
    handle_environment_numeric_inputs, setup_environment_numeric_inputs,
    sync_environment_numeric_inputs,
}; // 右栏只组合固定槽位，场景环境事务和天气预览值由独立模块处理。
pub(crate) use files::{atomic_write, safe_asset_path};
pub use files::workspace_asset_root; // 全局预设和scene_browser复用同一项目根、路径门与原子替换实现。
pub(crate) use live_density::{LiveBladeDensityState, sync_live_blade_density};
pub(crate) use live_water_dynamic::{LiveClipmapWaterState, sync_live_clipmap_water};
pub(crate) use load::{poll_draft_load, start_draft_load};
pub(crate) use paint::{apply_surface_brush, brush_overlaps_world_bounds};
pub(crate) use placement_grounding::{
    PlacementGroundingReadiness, PlacementTerrainFollowState, PlacementTerrainSampler,
    reconcile_following_placements, reconcile_record_with_terrain,
    set_height_mode_preserving_world_y, update_follow_offset_from_world_y,
}; // 变换UI、保存门与分帧任务共享唯一R16高度语义。
pub(crate) use portal_direction::sync_portal_directions_from_boundary; // 只在传送或边界修订变化时刷新未保存预览方向。
pub(crate) use preview::{
    clear_vegetation_previews,
    install_surface_preview_render, // 独立GPU候选通过编辑器同视口的真实管线验证。
    poll_surface_preview_rebuild,
    poll_vegetation_preview_rebuild,
    start_surface_preview_rebuild,
    start_vegetation_preview_rebuild,
    suppress_new_runtime_mesh_roots,
};
pub use resources::{EditorAuthoringSession, EditorSceneDraft};
pub(crate) use resources::{
    BoundaryEditState, BoundaryLiveOverrideState, DraftLoadTask, DraftSaveTask,
    EditorPreviewAssets, GlobalEnvironmentAuthoringState,
    GlobalEnvironmentSaveTask, PlacementWaterEvidenceRegistry, SurfaceBrushStroke,
    SurfacePreviewState, TerrainBrushStroke, TerrainLiveApplyState, TerrainPreviewState,
    VegetationBrushStroke, VegetationPreviewState, WaterBrushStroke, WaterLevelScrubState,
    WaterMaskMaterializeTask, WaterNetworkEditState, WaterPreviewState,
};
pub(crate) use save::transaction::commit_owned_files; // 全局目录和数组必须作为一批stage并可逆发布。
pub(crate) use save::{
    PortalPreviewHandoff, cleanup_saved_portal_previews, observe_added_placement_water_status,
    observe_removed_placement_water_status, observe_replaced_placement_water_status,
    poll_draft_save, poll_global_environment_save, register_placement_water_evidence_root,
    start_draft_save, start_global_environment_save, unregister_placement_water_evidence_root,
}; // 插件同时安装保存系统和其事件驱动证据索引生命周期。
pub(crate) use sparse_terrain::{
    SparseTerrainAuthoringState, apply_sparse_terrain_brush, apply_sparse_terrain_undo_redo,
    poll_sparse_building_history, poll_sparse_terrain_task, prefetch_sparse_terrain_cursor,
    start_sparse_terrain_save, sync_sparse_terrain_authoring,
}; // 新图局部高度独立于旧GLB作者管线，并通过单飞任务接入F12。
pub(crate) use surface_layer_edit::{
    // 引入地表作者层事务所需类型和纯助手。
    // UI与插件共享唯一作者删层入口和后台生命周期。
    SurfaceLayerEditTask,       // 保持当前地表操作的数据和原子采用边界。
    poll_surface_layer_edit,    // 完成当前地表作者事务的受检参数或值转换。
    start_surface_layer_delete, // 删除由稳定层身份发起，完成后统一进入撤销和保存。
}; // 大型像素只在任务内部共享或后台处理。
#[cfg(any(test, feature = "runtime_test_support"))]
pub(crate) mod test_support;
#[cfg(test)]
pub(crate) use test_support::test_heightfield_draft; // UI保存门测试复用同一实际拓扑动态草稿夹具。
#[cfg(test)]
mod brush_parity_tests; // 新旧编辑器共同输入的生产笔刷取证，不进入发布或普通运行路径。
pub(crate) use terrain::{
    apply_terrain_brush, poll_terrain_preview_rebuild, start_terrain_preview_rebuild,
    sync_terrain_preview_visibility,
};
pub use terrain_external::TerrainExternalChangeState;
pub(crate) use terrain_external::{
    TerrainConflictResolution, sync_external_terrain_changes,
};
pub(crate) use terrain_live::{clear_terrain_live_apply, maintain_terrain_live_apply};
pub use terrain_live::{TerrainLiveAdoption, TerrainLiveReceipt, TerrainLiveSet};
#[cfg(any(test, feature = "runtime_test_support"))]
pub use terrain_live::test_support as terrain_live_test_support;
pub(crate) use vegetation_compile::{
    VegetationCompileState, poll_vegetation_compile, start_vegetation_compile,
};
#[cfg(test)]
pub(crate) use vegetation_compile::VegetationCompileSource; // 空图捕获回归核对真实编译来源门，不公开内部编译模块。
pub(crate) use vegetation_layers::selected_vegetation_layer;
pub(crate) use vegetation_paint::apply_vegetation_brush;
pub(crate) use water_defaults::{default_water_asset, default_water_layer};
#[cfg(test)]
pub(crate) use water_defaults::water_asset_with_layer_for_tests; // 测试必须显式选择空水域或含层夹具。
pub(crate) use water_handoff::{
    WaterFileHandoff, cleanup_placement_handoff_previews, poll_water_file_handoff,
};
pub(crate) use water_level::{
    abort_water_level_scrub, apply_water_level_requests, apply_water_level_scrub,
    begin_water_level_scrub, finish_water_level_scrub, set_water_level,
}; // 数值框和视口Y轴手柄共用同一水位写入、连接同步与整笔历史事务。
pub(crate) use water_network::{
    WaterNetworkNumericInputSlot, WaterNetworkNumericInputState, WaterNetworkNumericKind,
    apply_water_network_actions, draw_water_network, edit_water_network_from_viewport,
    handle_water_network_numeric_inputs, setup_water_network_numeric_inputs,
    sync_water_network_connections, sync_water_network_numeric_inputs, sync_water_network_text,
    validate_water_network_edit,
};
pub(crate) use water_paint::apply_water_brush;
pub(crate) use water_preview::{
    clear_water_previews, poll_water_preview_rebuild, start_water_preview_rebuild,
    suppress_new_runtime_water_surfaces,
};
