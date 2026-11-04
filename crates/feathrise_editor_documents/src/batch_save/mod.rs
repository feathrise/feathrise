//! 私有文档界面构造，与原业务共享组件身份。
pub(crate) mod components {
    pub(crate) use feathrise_editor_documents_model::batch_save::components::*;
}
mod scene;
pub(crate) use scene::{button, report};
