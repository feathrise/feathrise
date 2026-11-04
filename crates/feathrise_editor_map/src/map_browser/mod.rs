//! F12左栏地图浏览器功能域。
//!
//! 本域只提供编辑器虚拟文件夹树、展开投影、拖拽和独立持久化；真实地图资产目录与运行时协议保持不变。

mod helpers; // 普通文档校验、树整理、会话定位和拖拽阈值纯函数。
#[cfg(test)]
mod helpers_tests; // 纯格式、排序、展开和拖拽规则使用独立测试文件，避免帮助函数单文件过长。
#[cfg(test)]
mod preview_tests; // 真实中文排版与窗口/UI缩放验证拖拽提示，防止空方框和坐标漂移。
mod index; // 大目录分帧快照、后台完整树与展开投影。
mod input; // 树展开、中文模态输入和六物理像素拖拽状态机。
mod persistence; // 编辑器专用Asset加载、并发检测与原子事务。
mod presentation; // 模态、浮层和有效落点的增量回显。
pub(crate) mod rename; // 地图名称与备注事务使用稳定身份及共享写入互斥。
pub(crate) use rename::poll_map_edit; // 单次结果同时更新目录、清单和当前草稿。
mod scene; // 文件夹模态框和穿透命中的拖拽浮层。
mod selection; // Ctrl会话选集与按当前可见顺序的后台Shift连选。
mod batch; // 混合选集去重、整批校验与共用受检写入事务。
pub(crate) use batch::poll_map_folder_batch;

pub(crate) use helpers::{
    expand_folder_ancestors, map_is_visible_in_tree, selected_container_folder_id,
}; // 作者创建与删除只复用树上下文纯函数，不依赖浏览系统内部状态机。
#[cfg(test)]
pub(crate) use helpers::{build_browser_tree, decode_folder_document}; // 目录截图复用正式整理与校验，避免另造一套树数据。
pub(crate) use index::advance_map_browser_index; // 插件Tasks阶段驱动有界索引。
pub(crate) use input::{
    MapBrowserClick,
    handle_map_browser_input, handle_map_folder_dialog_input, map_browser_general_input_available,
    navigate_map_browser_drag, update_map_browser_drag,
}; // Input阶段先推进归类所有权，再决定其他作者系统是否可运行。
pub(crate) use persistence::{
    MapFolderAssetLoader, apply_map_folder_asset_events, load_map_folder_asset,
    poll_map_folder_save, prepare_map_assignment_mutation, prepare_map_assignment_removal,
}; // 作者创建/删除与左栏结构操作共享唯一归类文件事务。
pub(crate) use presentation::{
    position_map_browser_drag_preview, sync_map_browser_drag_preview, sync_map_browser_root_row,
    sync_map_folder_dialog,
}; // Presentation只原地更新静态BSN实体。
pub(crate) use scene::{map_browser_drag_preview, map_folder_dialog}; // 静态BSN由主编辑器场景一次组合。
