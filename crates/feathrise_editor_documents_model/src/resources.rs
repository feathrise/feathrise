//! 根应用注入的文档场景工厂和纯内容操作；不接管World或追加调度阶段。
use bevy::prelude::*;
#[derive(Resource, Clone, Copy)]
pub struct DocumentScenes {
    pub panel: fn() -> Box<dyn Scene>,
    pub center_host: fn() -> Box<dyn Scene>,
    pub sidebar_launch: fn() -> Box<dyn Scene>,
    pub open_selected_button: fn() -> Box<dyn Scene>,
    pub search_panel: fn() -> Box<dyn Scene>,
    pub replace_tab: fn() -> Box<dyn Scene>,
}
/// 文档内容算法的稳定操作表；地图宿主不依赖文档布局和解析实现的编译产物。
/// 只传入共享文本与值类型，不能持有World、路径状态或另建历史/任务队列。
#[derive(Resource, Clone, Copy)]
pub struct DocumentContentOperations {
    pub parse_preview: fn(super::types::DocumentFormat, &[std::sync::Arc<str>]) -> super::types::ParsedPreview,
    /// 参数依次为完整LF正文、原CRLF换行标记、原BOM标记和可选格式化选区。
    pub process_json: fn(&str, bool, bool, Option<super::history::types::Selection>) -> super::json::types::ProcessedJson,
}
