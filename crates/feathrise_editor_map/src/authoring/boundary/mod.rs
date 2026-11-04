//! F12区域边界绘制、控制点编辑、可视预览和未保存碰撞同步。

mod cursor; // Clipmap查询按稳定墙ID和拖点编号冻结作者用途。
mod edit; // 重画、落点、拖点、闭合、墙厚与统一撤销操作。
mod helpers; // 命中半径、开放中间态校验与GLB派生高度贴地显示纯函数。
mod numeric_input; // 墙厚复用公共专业数值框与单次碰撞重建事务。
pub(crate) mod objects; // 独立空气墙对象列表与单墙管理。
pub(crate) mod path_history;
mod preview; // Gizmo、状态文本和正式运行时未保存覆盖。 // 单对象作者变更与撤销共享身份检查。

pub(crate) use cursor::clipmap_cursor_target;
pub(crate) use edit::{apply_boundary_actions, edit_boundary_from_viewport}; // 插件输入与作者阶段注册入口。
pub(crate) use numeric_input::{
    BoundaryThicknessInputSlot, BoundaryThicknessInputState, handle_boundary_thickness_input,
    setup_boundary_thickness_input, sync_boundary_thickness_input,
}; // 主界面只组合槽位，稳定路径事务和独立灵敏度由边界领域拥有。
pub(crate) use preview::{draw_map_boundaries, sync_boundary_text, sync_live_boundary_override}; // 插件显示与运行时同步阶段注册入口。
