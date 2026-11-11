//! F12静水、河流与瀑布共用的固定对象列表。

mod helpers; // 纯值分页、稳定ID解析、行摘要和选择变换。
mod systems; // 固定行池生成、回显、交互样式与点击系统。
#[cfg(test)]
pub(crate) mod source_selection_tests; // 根水与同ID私有水必须按完整来源恢复、拒绝和隔离历史。

pub(crate) use helpers::{activate_water_feature, activate_listed_water_feature, focus_author_source, listed_water_feature_at, WaterSourceSelectionGuard}; // 中央视口、列表和场景树复用来源与稳定ID激活逻辑。
pub(crate) use systems::{
    handle_water_feature_page_buttons, handle_water_feature_row_clicks, spawn_water_feature_rows,
    sync_water_feature_row_interactions, sync_water_feature_rows,
}; // 插件分别在Input与Presentation阶段注册有限行池系统。

/// 固定池最多显示六行且容器随当前页实际行数增长；最多72个水域对象通过显式分页直选。
pub(crate) const WATER_FEATURE_ROW_COUNT: usize = 6;
