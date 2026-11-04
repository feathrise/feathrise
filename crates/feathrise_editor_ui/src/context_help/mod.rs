//! 跨编辑器分库共用的问号与隐藏正文契约；根应用负责唯一浮层及鼠标归属。

mod components;
mod scene;

pub use components::{ContextHelpBody, ContextHelpButton};
pub use scene::{help_button, section_header, spawn_help_button};
