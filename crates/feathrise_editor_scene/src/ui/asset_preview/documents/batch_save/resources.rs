//! 批量保存只保留最多八份轻量结果；文件IO句柄仍由文档原单槽持有。

use super::types::FileReport;
use bevy::prelude::*;

#[derive(Resource, Default)]
pub(crate) struct DocumentBatchSaveState {
    pub(super) ticket: u64,
    pub(super) running: bool,
    pub(super) scope: String,
    pub(super) hint: String,
    pub(super) files: Vec<FileReport>,
    pub(super) shown: bool,
    pub(super) reset_scroll: bool,
    pub(super) wheel_owned: bool,
}

impl DocumentBatchSaveState {
    /// 工程关闭只观察原逐文件回执；部分成功不授权退出，失败正文仍留在原会话。
    pub(crate) fn project_close_result(&self) -> Result<bool, String> {
        if self.running { return Ok(false); }
        let failures = self.files.iter()
            .filter(|file| file.outcome == super::types::FileOutcome::Failed)
            .map(|file| format!("{}：{}", super::super::source::display_path(&file.path), file.detail))
            .collect::<Vec<_>>();
        if failures.is_empty() { Ok(true) }
        else { Err(format!("部分文档保存失败，草稿保留：{}", failures.join("；"))) }
    }

    pub(crate) fn exit_pending(&self) -> bool { self.running }

    pub(crate) fn owns_wheel(&self) -> bool {
        self.wheel_owned
    }
}
