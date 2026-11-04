//! 工程菜单只有一个后台保存槽；退出门可观察完成与失败，不能丢弃在途写入。

use bevy::{prelude::*, tasks::Task};

#[derive(Resource)]
pub(super) struct ProjectMenuSettings {
    pub project: crate::project::Project,
    pub managed: bool,
}

#[derive(Resource, Default)]
pub(crate) struct NativeProjectMenuState {
    pub(super) open: bool,
    pub(super) task: Option<Task<Result<super::store::SavedStartup, String>>>,
    pub(super) status: String,
    pub(super) failed: Option<String>,
    pub(super) pending_close: bool,
}

impl NativeProjectMenuState {
    pub(crate) fn is_open(&self) -> bool { self.open }

    pub(crate) fn saving(&self) -> bool {
        self.task.is_some()
    }
    pub(crate) fn save_failed(&self) -> Option<&str> {
        self.failed.as_deref()
    }

    /// 失败是受理事务的实际回执；关闭等待者也必须能看到，不能仅更新菜单局部文案。
    pub(crate) fn report_save_failure(&mut self, error: String) {
        self.failed = Some(error.clone());
        self.status = format!("启动设置未保存：{error}；原启动设置和作者草稿保留，可重试");
    }
}
