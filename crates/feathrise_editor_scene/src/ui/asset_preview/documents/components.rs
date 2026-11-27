//! 文档界面与业务共享同一稳定契约，不创建重复组件身份。
pub(crate) use feathrise_editor_documents_model::components::*;

/// 公共工具栏只绑定当前文件身份；切标签或重载换代会作废已经按下的旧手势。
#[derive(bevy::prelude::Component, Default)]
pub(super) struct SharedDocumentTarget(pub Option<super::types::SourceKey>);
