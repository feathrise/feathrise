//! 工程导出包的后台文件事务：流式复制、逐文件摘要、来源复核和完整目录发布。

mod files;
mod manifest;
mod paths;
mod transaction;
#[cfg(test)]
mod tests;

use super::types::ExportReceipt;
use crate::{build_process::BuildControl, native_play::PreparedGame, project::Project};
use std::{path::Path, sync::mpsc::SyncSender};

/// 只在导出工作线程调用；完整复制和校验不能进入作者帧或阻塞编辑器输入。
pub(crate) fn export(
    project: &Project,
    prepared: &PreparedGame,
    destination: &Path,
    control: &BuildControl,
    progress: &SyncSender<String>,
) -> Result<ExportReceipt, String> {
    control.check()?;
    if crate::project::open(&project.root)? != *project {
        return Err("工程清单已变化，请重新打开工程后导出".into());
    }
    let bundle = prepared.distribution_bundle(control)?;
    export_bundle(project, bundle, destination, &prepared.target, control, progress)
}

fn export_bundle(
    project: &Project,
    bundle: &crate::build_artifacts::PublishedBundle,
    destination: &Path,
    target: &Path,
    control: &BuildControl,
    progress: &SyncSender<String>,
) -> Result<ExportReceipt, String> {
    control.check()?;
    let destination = paths::destination(project, destination, target)?;
    let previous = transaction::existing(&destination, project, control)?;
    // 首次与更新发布的rename都可能通知原生监听；只在目标路径、现有内容均核对后排除精确目标。
    let mut output_lease = if paths::contains(&project.root.join("output"), &destination) { None }
        else { control.register_output_target(&destination)? };
    let _ = progress.try_send("正在收集已保存场景与运行文件".into());
    let plan = files::Plan::collect(project, bundle, control)?;
    let mut stage = transaction::Stage::create(&destination)?;
    let result = (|| {
        let _ = progress.try_send(format!("正在复制 {} 个文件（{:.1} MiB）", plan.files.len(), plan.bytes as f64 / 1048576.0));
        plan.copy(&stage.path, control, progress)?;
        let marker = manifest::write(project, bundle, &plan, &stage.path, control)?;
        let metadata_bytes = std::fs::metadata(stage.path.join(manifest::MARKER)).map_err(|error| error.to_string())?.len()
            + std::fs::metadata(stage.path.join(manifest::MANIFEST)).map_err(|error| error.to_string())?.len();
        let _ = progress.try_send("正在校验导出文件与已保存来源".into());
        manifest::verify(&stage.path, project, control, true)?;
        plan.verify_sources(project, bundle, target, control)?;
        control.check()?;
        let _ = progress.try_send("正在发布完整导出目录".into());
        let warning = stage.publish(&destination, previous.as_ref(), project, control)?;
        if let Some(lease) = output_lease.take() { lease.published(marker.export_id.clone()); }
        Ok(ExportReceipt {
            executable: destination.join(marker.executable),
            directory: destination,
            files: plan.files.len() + 2,
            bytes: plan.bytes + metadata_bytes,
            warning,
        })
    })();
    // 守卫仅回收 create_dir 成功的本轮 UUID 暂存目录；失败时从不删除作者选择的目标。
    result
}
