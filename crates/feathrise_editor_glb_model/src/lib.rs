//! 场景与 GLB 后台共享的请求和结果。任务槽和结果采用由场景所有。

use std::{future::Future, pin::Pin};

pub mod rigging;
pub mod weights;

/// 在原有后台工作槽中等待的结果，具体 Future 不参与场景库的编译。
pub type GlbTask<T> = Pin<Box<dyn Future<Output = Result<T, String>> + Send>>;
