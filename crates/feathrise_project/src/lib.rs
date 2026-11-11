//! 正式编辑器的工程内容上下文。复用生产场景格式、受检IO和预设校验，编辑器与工程运行时共用同一来源。
//! 本层不持有World、撤销栈或窗口；规模相关读取只允许调用方在后台工作槽执行。

mod scenes;
pub mod seed;
pub use scenes::{SceneEntry, ProjectScenes};
#[cfg(test)]mod tests;
