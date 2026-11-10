//! 资产直接路径引用：JSON字段和项目Rust字符串共用有界后台查询，不追踪预设ID依赖。

#[cfg(test)]
mod gpu_tests;
mod helpers;
mod scanner;
mod source_scanner; // 官方Rust词法与字符串解码，排除注释及文档属性。
#[cfg(test)]
mod source_tests;
mod sources; // 当前项目源码受检遍历，仍串行占用既有搜索槽。
#[cfg(test)]
mod tests;
mod types;

#[cfg(test)]
pub(in super::super) use gpu_tests::gpu_assert_references;
pub(super) use helpers::{accepts, in_scope, valid_target};
pub(super) use scanner::{collect_json, match_json};
pub(super) use sources::scan_sources;
