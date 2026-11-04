//! F12当前地图源传送点与地图树目标落点的双完整列表。

mod helpers; // 目标地图选择、资产解析和紧凑行文案。
mod input; // 目标有效性收口、双列表点击与当前源点删除。
mod name_edit; // 当前源点名称的中文输入、撤销提交和内联回显。
mod navigation; // 页面可见性与全局Select/Edit写入模式解耦。
mod presentation; // 按真实数量维护行实体、显隐、回显和交互样式。

pub(crate) use helpers::PortalDisplayContext; // 中央视口选择徽标与列表共用同一传送点名称解析。
pub(crate) use navigation::PortalSidebarNavigation; // 官方静态名称绑定与列表共用同一个可见页面门。

pub(crate) use input::{
    handle_portal_delete_button, handle_portal_list_row_clicks, reconcile_portal_selection,
}; // Input阶段只处理明确边沿和目标身份收口。
pub(crate) use name_edit::{handle_portal_name_editor, sync_portal_name_editor}; // 名称输入与Presentation保持独立职责。
pub(crate) use presentation::{
    PortalRowEntityRevision, spawn_portal_list_rows, sync_current_portal_row_interactions,
    sync_current_portal_rows, sync_portal_selection_panel, sync_target_portal_row_interactions,
    sync_target_portal_rows,
}; // Presentation阶段按作者数据完整更新两套列表与摘要。
