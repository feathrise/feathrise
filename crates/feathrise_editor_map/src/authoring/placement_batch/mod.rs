//! 跨地图对象剪贴板和普通资产批量复制/删除；一个CPU槽、完整校验、单条历史。

mod components;
mod paste;
mod resources;
mod scene_menu;
pub(crate) use scene_menu::input as handle_scene_menu;
mod systems;
mod types;
mod worker;
use super::placement_pose as terrain_patch_pose; // 复制与直接编辑使用同一精确姿态边界。

pub(crate) use components::PastePreviewGroup;
pub(crate) use components::PlacementBatchButton;
pub(crate) use resources::PlacementBatchState;
pub(crate) use systems::handle_placement_batch;
pub(crate) use systems::retire_empty_paste_group;
pub(crate) use systems::sync_batch_selection;
pub(crate) use types::PlacementBatchAction;

#[cfg(test)]
mod tests;
