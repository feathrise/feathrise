//! 作者与工程任务的原生进程边界；平台后端共用精确所有权、独立协议管道与非阻塞收口契约。

#[cfg(windows)]
mod windows;
#[cfg(unix)]
mod unix;

#[cfg(windows)]
pub use windows::{JobProcess, child_channels};
#[cfg(windows)]
pub(crate) use windows::SpawnFailure;
#[cfg(unix)]
pub use unix::{JobProcess, child_channels};
#[cfg(unix)]
pub(crate) use unix::SpawnFailure;
