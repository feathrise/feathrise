//! 函数体热补丁的后台兼容与产物边界；作者状态和原完整构建／重启链继续由原模块持有。

pub(crate) mod compatibility;
pub(crate) mod compiler_output;
// 现有dx／COFF补丁链只验收Windows；Unix普通代码迭代继续完整构建与重启。
#[cfg(windows)]
pub(crate) mod toolchain;
#[cfg(windows)]
pub(crate) mod artifacts;
mod types;
pub(crate) use types::{FatBaseline, PatchContext, PatchTarget, PreparedPatch};
