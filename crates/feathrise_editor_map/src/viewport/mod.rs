//! 独立离屏三维编辑视口和CAD式导航。

mod blade_view; // 打开F12时临时把唯一正式GpuBladeView搬到编辑相机。
mod camera_activity; // 独立于作者文档门关闭旧离屏相机，避免抢占游戏LOD。
pub(crate) use camera_activity::sync_editor_camera_activity;
mod brush_cursor; // 外圈显示世界米制半径、内圈显示强度，只渲染到F12离屏相机。
pub(crate) mod tool_preview; // 原工具发布语义状态，两种视口共用几何生成器。
pub(crate) mod spatial_brush; // 固定空间笔刷与起终点直线；摆放预览和密度执行分离。
mod camera_math; // 三维地图取景、正交深度与四种视图姿态的纯数学。
pub(crate) mod framing; // 聚焦所选/查看全图复用完整选择范围与当前观察轴。
pub(crate) mod content_framing; // 按固定预算捕获当前内容，后台统计GPU地形真实范围。
pub(crate) mod content_selection; // 作者层候选与各类别高亮共享稳定选择身份。
pub(crate) mod modes; // 全视口选择/移动/编辑模式与固定类别筛选，不隶属于地形工具。
pub(crate) mod orientation_axis; // 地图和模型预览复用方向轴控件，各自控制原相机。
pub(crate) mod shading; // 四球显示只作用于当前地图离屏相机。
mod view_controls_input; // 方向球与四球共用文档、模态、外部拖动和数值焦点门。
mod placement_selection; // Scene0网格单击精确高亮、中央徽标和双击相机定位。
pub(crate) mod placement_move; // 世界轴/XZ平面移动与输入捕获；建筑松手复用原后台地基验证。
pub(crate) mod multi_select; // 普通资产的临时选择集合、拖框与单次整组移动。
pub(crate) mod snapping; // 同排吸附偏好、局部参考网格和真实地形落点。
mod portal_labels; // F12独立相机接管正式与未保存传送标签的朝向、距离和可见性。
mod portal_selection; // 当前传送点单击世界标记、中央徽标和双击相机定位。
mod surface_selection; // 选中作者层按真实索引权重显示青色地皮。
mod surface_trail; // 地表绘制立即显示固定256²柔边覆盖，PBR采用后退出。
pub(crate) mod sub_maps; // 完整引用的整体位置与朝向手柄。
pub(crate) mod terrain_objects; // 独立地形的真实表面点选与范围高亮。
mod terrain_transform; // 单对象实时姿态预览和松手单条历史，复用已有变换数学。
pub(crate) use terrain_transform::transaction_active as terrain_transform_transaction_active;
mod systems; // RTT创建、地图取景、投影切换、平移旋转缩放与光标射线。
#[cfg(test)]
pub(crate) use systems::editor_camera_pose; // 离屏验收复用正式视口矩阵，避免测试另造相机而遮蔽拾取偏差。
pub(crate) mod source_fence; // 父图世界编辑避开只读子地图，完整空间索引由单飞后台构建。
mod view_memory; // 切图按已采用视口身份记忆相机，不依赖旧RTT继续存活。
pub(crate) use surface_selection::{SurfaceSelectionOverlay, sync_surface_selection_overlay}; // 独立single-flight选中覆盖。
mod water_selection; // 三类水域对象单击精确高亮、中央徽标和双击相机定位。
mod water_transform; // 移动工具的静水Y轴手柄复用原水位事务，不重采样XZ遮罩。
pub(crate) use water_transform::transaction_active as water_transform_transaction_active;
pub(crate) mod water_plane; // 静水水位平面射线与输入所有权独立于地形查询。

pub(crate) use blade_view::{EditorBladeViewHandoff, handoff_gpu_blade_view};
pub(crate) use camera_math::framing_extent; // 场景树作者层与原视口共享按方向/投影取景的数学。
pub use brush_cursor::MapEditorBrushGizmos;
pub(crate) use brush_cursor::{
    MAP_EDITOR_BRUSH_RENDER_LAYER, // 地表预热复用F12专用层，确保采用前已通过同视口管线。
    configure_map_editor_brush_gizmos,
    draw_map_editor_brush,
    map_editor_brush_cursor_visible,
    map_editor_brush_display_hit,
    map_editor_brush_follows_cursor,
};
pub(crate) use content_selection::select_editor_viewport_content; // 相机光标落点发布后执行一次点击边沿拾取。
pub use content_selection::LayerCandidateState;
pub(crate) use content_selection::LayerTarget; // 普通作者层选择与各模块高亮只在地图内部流转。
pub(crate) use content_selection::{
    apply_layer_candidate, layer_candidate_interactions, poll_layer_candidates,
    sync_layer_candidate_ui, sync_sidebar_layer_selection,
}; // 候选输入、后台采用和浮层显示按编辑器阶段注册。
pub(crate) use orientation_axis::{orientation_axis_input, setup_editor_orientation_axis, sync_editor_orientation_axis};
pub(crate) use placement_selection::{
    SelectedPlacementViewportState, draw_selected_placement_highlight_frame,
    resolve_selected_placement_mesh, setup_selected_placement_badge,
    sync_selected_placement_viewport,
};
pub(crate) use portal_labels::sync_editor_portal_labels; // PostUpdate以最终编辑相机姿态呈现传送文字。
pub(crate) use portal_selection::{
    SelectedPortalViewportState, draw_selected_portal_marker, setup_selected_portal_badge,
    sync_selected_portal_viewport,
};
pub(crate) use surface_trail::{SurfaceStrokeTrail, SurfaceTrailSource, draw_surface_stroke_trail}; // 输入与显示共享固定柔边覆盖图。
pub(crate) use systems::{
    control_editor_viewport, frame_selected_map, set_editor_view, setup_editor_viewport,
    sync_editor_viewport_camera, sync_editor_viewport_render_size,
};
pub(crate) use water_selection::{
    SelectedWaterViewportState, draw_selected_water_highlight_frame, setup_selected_water_badge,
    sync_selected_water_viewport,
};

pub(crate) mod vegetation_ground; // Clipmap植被精确地面single-flight查询。
pub(crate) use vegetation_ground::{VegetationGroundCursor, resolve_vegetation_ground_cursor}; // 场景发布后统一采用落点。
