//! 唯一搜索会话和串行IO/CPU工作槽；旧任务真正结束前不另开搜索。

use super::types::*;
use crate::ui::external_files::{ExternalEditor, ExternalEditors};
use bevy::{prelude::*, tasks::Task};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Resource)]
pub(crate) struct AssetSearchState {
    pub(super) open: bool,
    // 表单借给活动文件窗时，固定栏继续按原来的开合及按钮状态显示。
    sidebar_before_float: Option<(bool, bool)>,
    pub(super) query: String,
    pub(super) filter: String,
    pub(super) current_folder: bool,
    pub(super) host_folder: Option<String>, // 浮窗服务使用其目录；左栏和后台搜索仍共享唯一结果槽。
    pub(super) case_sensitive: bool,
    pub(super) whole_word: bool,
    pub(super) preferred_editor: Option<ExternalEditor>, // 本进程保留用户选择；切查询不改变偏好，也不写项目设置。
    pub(super) reference_target: Option<String>,
    pub(super) missing_assets: bool,
    pub(super) reference_requested: bool, // 两类路径查询等鼠标释放、候选与粘贴确认后再冻结草稿。
    pub(super) reference_root: Option<std::path::PathBuf>, // 排队期间换根不能将旧目标用于另一资产目录。
    pub(super) ticket: u64,
    pub(super) pending: Option<SearchRequest>,
    pub(super) running: Option<SearchRequest>,
    pub(super) task: Option<Task<SearchResult>>,
    pub(super) result: Option<Arc<SearchResult>>,
    pub(super) jump: Option<PendingJump>,
    pub(super) reveal: Option<JumpReveal>,
    pub(super) wheel_owned: bool,
    pub(super) reset_scroll: bool,
    pub(super) hint: String,
}

impl Default for AssetSearchState {
    fn default() -> Self {
        Self {
            open: false,
            sidebar_before_float: None,
            query: String::new(),
            filter: String::new(),
            current_folder: false,
            host_folder: None,
            case_sensitive: true,
            whole_word: false,
            preferred_editor: None,
            reference_target: None,
            missing_assets: false,
            reference_requested: false,
            reference_root: None,
            ticket: 0,
            pending: None,
            running: None,
            task: None,
            result: None,
            jump: None,
            reveal: None,
            wheel_owned: false,
            reset_scroll: false,
            hint: String::new(), // 搜索教程由标题旁?展示，初始状态没有真实操作回执。
        }
    }
}

impl AssetSearchState {
    pub(crate) fn is_open(&self) -> bool { self.open }
    pub(crate) fn is_missing_assets(&self) -> bool { self.missing_assets }
    pub(crate) fn floating_hosted(&self) -> bool { self.sidebar_before_float.is_some() }

    pub(crate) fn set_floating_host(&mut self, hosted: bool) {
        if hosted && self.sidebar_before_float.is_none() {
            self.sidebar_before_float = Some((self.open, self.missing_assets));
            self.open = false;
        } else if !hosted && let Some((open, _)) = self.sidebar_before_float.take() {
            // 归还可见状态，保留唯一查询结果与票据供已打开的中央替换预览继续使用。
            self.open = open;
        }
    }

    pub(super) fn sidebar_display(&self) -> (bool, bool) {
        self.sidebar_before_float.unwrap_or((self.open, self.missing_assets))
    }
    #[cfg(feature = "validation")]
    pub(crate) fn validation_result(&self) -> Option<(usize, Vec<String>)> {
        let result = self.current()?;
        Some((result.hits, result.files.iter().map(|file| file.path.clone()).collect()))
    }
    pub(in super::super) fn selected_editor(&self, available: &ExternalEditors) -> Option<ExternalEditor> {
        // 先用用户选择；入口被卸载时回退到仍可用的IDE，不能让旧缓存绕过安装门。
        self.preferred_editor.filter(|editor| available.installed(*editor))
            .or_else(|| ExternalEditor::ALL.into_iter().find(|editor| available.installed(*editor)))
    }

    #[cfg(test)]
    pub(crate) fn requested_reference_target(&self) -> Option<&str> {
        self.reference_target.as_deref().filter(|_| self.reference_requested)
    }

    pub(crate) fn request_references(&mut self, path: String, root: std::path::PathBuf) {
        if !super::references::valid_target(&path) {
            return;
        }
        self.invalidate("等待输入确认后查看直接引用…");
        self.missing_assets = false;
        self.reference_target = Some(path);
        self.reference_requested = true;
        self.reference_root = Some(root);
        self.open = true;
    }

    pub(super) fn request_missing_assets(&mut self, root: std::path::PathBuf) {
        self.invalidate("等待输入确认后检查资产路径…");
        self.reference_target = None;
        self.missing_assets = true;
        self.reference_requested = true;
        self.reference_root = Some(root);
        self.open = true;
    }

    pub(super) fn path_query(&self) -> bool {
        self.missing_assets || self.reference_target.is_some()
    }

    pub(super) fn text_search(&mut self) {
        if self.path_query() {
            self.invalidate("");
        }
        self.reference_target = None;
        self.missing_assets = false;
        self.open = true;
    }

    pub(crate) fn owns_wheel(&self) -> bool {
        self.wheel_owned
    }

    pub(super) fn current(&self) -> Option<&SearchResult> {
        self.result
            .as_deref()
            .filter(|result| result.ticket == self.ticket && !result.cancelled
                && result.references == self.path_query() && result.missing_assets == self.missing_assets)
    }

    pub(super) fn invalidate(&mut self, hint: &str) {
        self.ticket = self.ticket.wrapping_add(1);
        if let Some(request) = &self.running {
            request.cancel.store(true, Ordering::Relaxed);
        }
        self.pending = None;
        self.reference_requested = false;
        self.reference_root = None;
        self.jump = None;
        self.reveal = None;
        self.reset_scroll = true;
        self.hint = hint.into();
        // 旧结果保留共享所有权；下一次后台任务再释放，UI帧不析构整份分组/片段。
    }

    pub(super) fn cancellation() -> Arc<AtomicBool> {
        Arc::new(AtomicBool::new(false))
    }
}
