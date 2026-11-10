//! F12 对共享稀疏高度作者核心的薄适配层。
//!
//! 笔刷、分页缓存、世界采样与撤销数据均由 `feathrise_scene_content` 提供；本模块只保留
//! Bevy ECS、输入、单飞后台任务和 F12 历史/保存接线，未来独立编辑器无需依赖游戏 UI。

#[cfg(test)] // 缓存与候选预算只由适配层回归测试直接断言。
pub(crate) use feathrise_scene_content::{MAX_SPARSE_AUTHOR_CLEAN_TILES, MAX_SPARSE_BRUSH_CANDIDATES};
pub(crate) use feathrise_scene_content::{
    SparseTerrainAuthorSession, SparseTerrainBrushInput, SparseTerrainBrushKind,
    SparseTerrainHeightChange,
}; // 现有 F12 集成与回归测试经同一入口消费共享作者合同。

mod integration; // F12 资源、单飞任务和输入/保存/历史接线不进入可复用作者核心。
mod resources;
mod types; // 作者会话借用票据独立于ECS资源和计算流程。 // 保存系统参数组保护建筑域内部类型的可见性边界。
pub(crate) use integration::{
    SparseTerrainAuthoringState, apply_sparse_terrain_brush, apply_sparse_terrain_undo_redo,
    poll_sparse_building_history, poll_sparse_terrain_task, prefetch_sparse_terrain_cursor,
    start_sparse_terrain_save, sync_sparse_terrain_authoring,
};
pub(crate) use types::SparseTerrainAuthorLease; // 上层 authoring 插件只看见 F12 所需系统和状态资源。

#[cfg(test)] // 共享核心继续通过真实 F12 适配入口验证保存、历史、平滑与缓存边界。
mod tests;
