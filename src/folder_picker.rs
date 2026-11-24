//! 原生选择器的平台门面，统一工程目录、作者工具和单文件选择。

const PROJECT_ICON_TITLE: &str = "选择工程图标 / Select Project Icon";
const PROJECT_ICON_FILTER: &str = "图像 / Images (PNG, JPG, JPEG)";

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(crate) use windows::FolderPicker;

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub(crate) use unix::{FolderPicker, run_helper};
