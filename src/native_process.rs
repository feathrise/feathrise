//! 作者与工程任务的原生进程边界；平台后端共用精确所有权、独立协议管道与非阻塞收口契约。

#[cfg(windows)]
mod windows;
#[cfg(unix)]
mod unix;

#[cfg(windows)]
pub use windows::JobProcess;
#[cfg(windows)]
pub(crate) use windows::command_executable as resolve_tool;
#[cfg(all(windows, feature = "validation"))]
pub(crate) use windows::isolate_registry_for_validation;
#[cfg(unix)]
pub use unix::JobProcess;
#[cfg(unix)]
pub(crate) use unix::executable as resolve_tool;
