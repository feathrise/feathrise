//! 文档内容结果、固定组件与工厂契约；原生输入、草稿、任务槽仍只有宿主中的一份。
pub mod components;
pub mod constants;
mod resources;
pub mod types;
pub use resources::{DocumentScenes, DocumentContentOperations};
pub mod batch_save;
pub mod history;
pub mod json;
pub mod navigation;
pub mod search;

mod document_format;
pub use document_format::document_format;
