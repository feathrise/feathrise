//! 私有文档界面构造，与原业务共享组件身份。
pub(crate) mod components {
    pub(crate) use feathrise_editor_documents_model::search::components::*;
}
pub(crate) mod constants {
    pub(crate) use feathrise_editor_documents_model::search::constants::*;
}
pub(crate) mod replacement;
pub(crate) use replacement::panel as replacement_panel;
mod scene;
pub(crate) use scene::panel;
