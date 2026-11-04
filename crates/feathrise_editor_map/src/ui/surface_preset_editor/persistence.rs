//! 保留原 Tasks 阶段入口，后台任务/采用门/冻结回执均借用唯一地表会话。
pub(super) use super::systems::poll_catalog_write;
#[cfg(test)]
mod feedback_tests;
