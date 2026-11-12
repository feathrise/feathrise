//! assets目录后台浏览、类型识别和分页选择。

mod document_format; // 文档、源码与配置共享唯一格式白名单，保留二进制预览边界。
mod filter; // 类型与 GLB 片段元数据只在后台计算，两个呈现方式共享目录快照。
mod paths; // 资产路径交互输入规范化，不执行文件 IO。
pub(crate) mod project_files; // 工程文件视图复用单层扫描，拥有独立导航、选择和后台槽。
pub(crate) mod navigation; // 新旧工作台共用刷新、上级和分页语义。
mod resources; // 单一后台槽、当前完整目录和待采用定位意图。
mod reveal; // 引用反查请求、迟到保护与目录选择原子采用。
mod scan; // 不接触ECS的单层目录扫描。
mod systems; // 有界任务启动、非阻塞采用和行点击。
mod types; // 后台完成值与定位身份的纯数据。

#[cfg(test)]
mod system_tests; // 真实 ECS 任务槽验证取消、迟到和后台退役，不依赖 GPU。

pub(crate) use paths::normalize_asset_reference; // 路径行定位和赋值共用同一词法规则。
pub(crate) use document_format::document_format;
pub(crate) use filter::AssetTypeFilter;
pub(crate) use types::DocumentFormat;
pub(crate) use types::AssetCatalogFeedback;
pub use resources::EditorAssetCatalog; // 根资源模块继续重导该类型以保持既有调用入口。
pub use reveal::request_asset_reveal;
pub(crate) use reveal::request_asset_folder; // 任意预设引用与收藏只导航浏览器，不改作者内容。

pub(crate) use systems::{
    handle_asset_row_clicks, poll_asset_scan, request_initial_asset_scan, start_asset_scan,
};
