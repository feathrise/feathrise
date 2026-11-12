//! 动画作者文档的纯数据边界；读取、曲线计算和保真保存只供后台任务调用。

mod curves;
mod reader;
mod types;
mod writer;

pub use types::{
    AnimationDocument, Channel, Clip, Keyframe, Node,
};

#[cfg(any(test, feature = "runtime_test_support"))]
pub mod tests;
