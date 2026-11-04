//! 正式编辑器的工程内容上下文。复用生产地图格式、受检IO和预设校验，编辑器与工程运行时共用同一来源。
//! 本层不持有World、撤销栈或窗口；规模相关读取只允许调用方在后台工作槽执行。

mod maps;
pub mod seed;
pub use maps::{MapEntry, ProjectMaps, selection, set_selection};

/// 作者文档只记录当前地图选择；正式地图文件继续使用原有格式和稳定对象ID。
pub const MAP_SELECTION_KEY: &str = "production_map";
#[cfg(test)]mod tests;
