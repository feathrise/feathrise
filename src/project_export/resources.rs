//! 导出面板只保存短状态；单一后台工作槽拥有进程控制、进度邮箱和完成回执。

use super::types::ExportReceipt;
use crate::{build_process::BuildControl, project::Project};
use bevy::prelude::*;
use std::{path::PathBuf, sync::mpsc::Receiver, thread::JoinHandle};

#[derive(Resource)]
pub(crate) struct ExportState {
    pub(crate) open: bool,
    pub(crate) destination: String,
    pub(crate) status: String,
    pub(crate) failed: Option<String>,
    pub(crate) receipt: Option<ExportReceipt>,
    pub(crate) revision: u64,
    pub(super) working: bool,
}

impl ExportState {
    pub(crate) fn new(project: &Project) -> Self {
        Self {
            open: false,
            destination: crate::project::display_path(&project.root.join("output")),
            status: "导出当前工程的已保存内容".into(),
            failed: None, receipt: None, revision: 1, working: false,
        }
    }

    pub(crate) fn busy(&self) -> bool { self.working }
    pub(super) fn changed(&mut self) { self.revision = self.revision.wrapping_add(1).max(1); }
}

#[derive(Resource)]
pub(super) struct ExportSettings {
    pub(super) project: Project,
    pub(super) target: PathBuf,
}

#[derive(Default)]
pub(super) struct ExportRuntime {
    pub(super) active: Option<ActiveExport>,
    pub(super) stopping: bool,
    pub(super) failure: Option<String>,
}

pub(super) struct ActiveExport {
    pub(super) control: BuildControl,
    pub(super) progress: Receiver<String>,
    pub(super) result: Receiver<Result<ExportReceipt, String>>,
    pub(super) handle: JoinHandle<()>,
}
