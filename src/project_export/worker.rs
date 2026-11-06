//! 导出后台串联真实工程构建和完整包发布；不创建预览实例，不修改作者文档。

use super::types::ExportReceipt;
use crate::{build_process::BuildControl, project::Project};
use feathrise_editor_core::ProjectBuildProfile;
use std::{path::Path, sync::mpsc::SyncSender};

pub(super) fn run(
    project: &Project, target: &Path, destination: &Path,
    profile: ProjectBuildProfile, control: &BuildControl, progress: &SyncSender<String>,
) -> Result<ExportReceipt, String> {
    control.check()?;
    let _ = progress.try_send(format!("正在构建{}工程…", profile.label()));
    // 共享原构建单飞门、Cargo 配置与缓存；交付版本由导出专用链接策略构建。
    let before = crate::code_iteration::export_source_identity(project, target, control)?;
    let mut prepared = crate::native_play::prepare_export(project, target, profile, control)?;
    if crate::code_iteration::export_source_identity(project, target, control)? != before {
        return Err("构建期间工程源码已变化，请重新导出；原输出保留".into());
    }
    prepared.bind_build_metadata(serde_json::json!({
        "project_id": project.id, "project_root": project.root,
        "profile": profile.cargo_profile(), "export_source_identity": before,
    }), control)?;
    let _ = progress.try_send("构建完成，正在收集运行文件和已保存资产…".into());
    super::package::export(project, &prepared, destination, control, progress)
}
