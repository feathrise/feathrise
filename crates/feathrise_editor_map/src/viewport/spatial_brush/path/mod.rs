//! 空间路径节点编辑、后台预览与工具路径库，沿用原雕刻请求和输入所有权。
mod preview;
mod resources;
mod systems;
mod types;
mod ui;
pub(super) use preview::{draw, pick};
pub(crate) use resources::PathState;
pub(super) use systems::{build_request, drive};
pub(super) use ui::PathOnly;
#[cfg(test)]
pub(super) use ui::{PathButton, PathSize};
pub(super) use ui::{input, setup, sync};

#[cfg(test)]
mod tests;
