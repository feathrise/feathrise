//! 返回窗口时核对外部文件：干净会话自动刷新，草稿及未确认输入始终优先。

mod resources;
mod systems;
mod types;
#[cfg(test)]
mod tests;

pub(super) use resources::ExternalCheckState;
pub(super) use systems::{drive, request_check};
pub(super) use types::{CheckReply, ExternalDocument};
