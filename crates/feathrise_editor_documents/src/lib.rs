//! 文档格式、内容解析与作者界面；内容算法与布局均不依赖地图宿主。
use bevy::prelude::*;
use feathrise_editor_documents_model::DocumentScenes;
mod components {
    pub(crate) use feathrise_editor_documents_model::components::*;
}
mod constants {
    pub(crate) use feathrise_editor_documents_model::constants::*;
}
mod preview;
pub use feathrise_editor_documents_model::document_format;
pub use preview::parse as parse_preview;
mod batch_save;
mod history;
pub mod json;
mod minimap;
mod navigation;
mod scene;
mod search;
pub struct DocumentsEditorPlugin;
impl Plugin for DocumentsEditorPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(scenes()).insert_resource(content_operations());
    }
}
/// 测试与根程序共用实际工厂，所有返回场景保留原字体依赖注册和实体树。
pub fn scenes() -> DocumentScenes {
    DocumentScenes {
        panel: || Box::new(scene::panel()),
        center_host: || Box::new(scene::center_host()),
        sidebar_launch: || Box::new(scene::sidebar_launch()),
        open_selected_button: || Box::new(scene::open_selected_button()),
        search_panel: || Box::new(search::panel()),
        replace_tab: || Box::new(search::replacement::tab()),
    }
}

/// 正式宿主与测试都注入同一组纯内容算法，地图调度只复制这个固定大小函数表。
pub fn content_operations() -> feathrise_editor_documents_model::DocumentContentOperations {
    feathrise_editor_documents_model::DocumentContentOperations {
        parse_preview,
        process_json: json::process,
    }
}
