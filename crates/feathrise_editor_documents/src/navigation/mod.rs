//! 私有文档界面构造，与原业务共享组件身份。
pub(crate) mod components {
    pub(crate) use feathrise_editor_documents_model::navigation::components::*;
}
pub(crate) mod constants {
    pub(crate) use feathrise_editor_documents_model::navigation::constants::*;
}
mod scene;
pub(crate) use scene::{gutter, match_layer, tools};
