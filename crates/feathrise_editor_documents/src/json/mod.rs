//! JSON内容处理与私有界面构造；不依赖地图状态、文件事务或原生输入调度。
pub(crate) mod components {
    pub(crate) use feathrise_editor_documents_model::json::components::*;
}
mod scene;
pub(crate) use scene::{diagnostic, format_button};

mod format;
mod processing;
mod validation;
#[cfg(test)]
mod tests;
pub use processing::{format, process};
pub use validation::validate;
pub use feathrise_editor_documents_model::json::supported;
