//! 代码迭代后台边界：监听与完整构建分成固定两槽，作者帧只读取原子身份和有界邮箱。

mod artifacts;
mod output_scope;
mod resources;
mod source;
mod types;
mod worker;

pub(crate) use resources::Backend;
pub(crate) use types::{BuildIdentity, Event, PreparedVersion, PreparedArtifact};
pub(crate) use source::{SourceSnapshot, capture as capture_source};

/// 导出在后台复用同一套源码和工具链身份算法，不另扫一份规则不同的工程清单。
pub(crate) fn export_source_identity(project: &crate::project::Project, target: &std::path::Path,
    control: &crate::build_process::BuildControl) -> Result<String, String> {
    let source = source::capture(project, target, control)?;
    Ok(format!("{}:{}", source.sha256, source.environment_sha256))
}
