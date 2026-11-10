//! 唯一替换槽、最新请求与中央预览标签；失效结果留待后台退役，草稿历史归原文档。

use super::types::*;
use crate::ui::vegetation_preset_editor::PresetWorkspaceTab;
use bevy::{prelude::*, tasks::Task};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Resource)]
pub(crate) struct AssetReplaceState {
    pub(super) tab_open: bool,
    pub(super) active: bool,
    pub(super) return_tab: PresetWorkspaceTab,
    pub(super) center_activation: u64,
    pub(super) replacement: String,
    pub(super) ticket: u64,
    pub(super) preview_ticket: u64,
    pub(super) search_ticket: Option<u64>,
    pub(super) checked: Vec<usize>,
    pub(super) file: usize,
    pub(super) hit: usize,
    pub(super) plan: Option<Arc<ReplacePlan>>,
    pub(super) applied: bool,
    pub(super) pending: Option<Request>,
    pub(super) running: Option<Request>,
    pub(super) task: Option<Task<Reply>>,
    pub(super) retired: Option<Outcome>,
    pub(super) retired_request: Option<Request>, // 重载后旧请求可能是全文最后一个Arc所有者，也由后台退役。
    pub(super) batch: Option<BatchHistory>,
    pub(super) restores: Vec<(
        super::super::types::DraftStamp,
        super::super::super::history::Selection,
    )>, // 只暂存至多8份选区，等唯一原生输入重新绑定后恢复。
    pub(super) wheel_owned: bool,
    pub(super) reset_files: bool,
    pub(super) reset_diff: bool,
    pub(super) hint: String,
}

impl Default for AssetReplaceState {
    fn default() -> Self {
        Self {
            tab_open: false,
            active: false,
            return_tab: PresetWorkspaceTab::Scene,
            center_activation: 0,
            replacement: String::new(),
            ticket: 0,
            preview_ticket: 0,
            search_ticket: None,
            checked: Vec::new(),
            file: 0,
            hit: 0,
            plan: None,
            applied: false,
            pending: None,
            running: None,
            task: None,
            retired: None,
            retired_request: None,
            batch: None,
            restores: Vec::new(),
            wheel_owned: false,
            reset_files: false,
            reset_diff: false,
            hint: String::new(), // 替换教程已收进预览标题旁?，底栏保留真实回执。
        }
    }
}

impl AssetReplaceState {
    /// 已受理的批量替换可能尚未采用到原文档草稿；退出不能抢先丢掉结果。
    pub(crate) fn exit_pending(&self) -> bool {
        self.pending.is_some() || self.running.is_some() || self.task.is_some()
    }

    pub(crate) fn owns_wheel(&self) -> bool {
        self.wheel_owned
    }
    pub(in super::super::super) fn preview_visible(&self) -> bool {
        self.tab_open && self.active
    }
    pub(super) fn current(&self) -> Option<&ReplacePlan> {
        self.plan
            .as_deref()
            .filter(|plan| plan.ticket == self.preview_ticket)
    }
    pub(super) fn cancel(&mut self, hint: &str) {
        self.ticket = self.ticket.wrapping_add(1);
        self.preview_ticket = self.ticket;
        if let Some(request) = &self.running {
            request.cancel.store(true, Ordering::Relaxed);
        }
        self.pending = None;
        self.applied = false;
        self.hint = hint.into();
        self.reset_diff = true;
    }
    pub(super) fn request(&mut self, work: Work) {
        self.cancel("正在后台准备；完成前可继续浏览或取消");
        if let Work::Apply { plan, .. } = &work {
            self.preview_ticket = plan.ticket;
        }
        self.pending = Some(Request {
            ticket: self.ticket,
            cancel: Arc::new(AtomicBool::new(false)),
            work,
        });
    }
}
