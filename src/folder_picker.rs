//! 原生选择器的平台门面；工程及导出选目录，作者插件只选当前平台单文件。

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(crate) use windows::FolderPicker;

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub(crate) use unix::{FolderPicker, run_helper};
