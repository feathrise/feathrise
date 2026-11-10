//! 普通摆放资产的临时多选；作者层与建筑规则继续使用原入口，不写入场景格式。

mod input;
mod ownership;
mod presentation;
mod resources;
#[cfg(test)]
mod source_frame_tests;
pub(crate) use input::{advance_multi_selection, handle_multi_select_pointer};
pub(crate) use input::{box_filter, publish_primary};
pub(crate) use ownership::sync_multi_selection_ownership;
pub(crate) use presentation::{
    MultiSelectionBadge, MultiSelectionRectangle, draw_multi_selection, setup_multi_selection_ui, sync_multi_selection_ui,
};
pub(crate) use resources::{PlacementSelectionSet, SelectedMember};
pub(super) const MAX_SELECTED: usize = feathrise_scene_content::MAX_COURTYARD_MEMBERS_PER_PRESET;
pub(super) const CAPTURE_BUDGET: usize = 128; // 与已有院落框选一致，只分帧读取作者原点。
pub(super) const REFRESH_BUDGET: usize = 16; // 结构或作者换代时局部复核已选集合，不扫描场景。
