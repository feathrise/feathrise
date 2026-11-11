//! F4 六个植被页的四档 LOD 性能隔离控件。
//! 【功能设计】界面与 F3 完全一致并共享唯一状态；开关作用于当前场景运行时与 F12 正式预览，
//! 但只临时控制匹配叶片窗口或Mesh LOD节点，不改写场景文档和持久化设置。

mod apply;
mod state;
mod ui;

pub(super) use apply::{apply_changed_vegetation_lods, apply_vegetation_lods_to_new_nodes};
#[cfg(test)]
pub(super) use ui::EditorVegetationLodControls;
pub(super) use ui::{
    handle_vegetation_lod_control_interactions, spawn_vegetation_lod_controls,
    update_vegetation_lod_control_ui,
};
