//! 主窗口的唯一工程准备槽、设置写入槽与候选所有权；不持有跨工程作者草稿。
use super::process::AuthorProcess;
use crate::{
    build_process::BuildControl, folder_picker::FolderPicker, project::Project,
    user_preferences::Preferences, workspace_ui::WorkspaceField,
};
use std::{path::PathBuf, sync::mpsc::Receiver, thread::JoinHandle, time::Instant};

pub(super) struct OpenJob {
    pub worker: JoinHandle<()>,
    pub result: Receiver<Result<Project, String>>,
    pub control: BuildControl,
}
pub(super) struct PreferencesJob {
    pub worker: JoinHandle<()>,
    pub result: Receiver<Result<Preferences, String>>,
}
pub(super) struct Picker {
    pub picker: FolderPicker,
    pub field: WorkspaceField,
    pub origin: Option<u32>,
}
pub(super) struct Host {
    pub target: PathBuf,
    pub preferences: Preferences,
    pub preferences_job: Option<PreferencesJob>,
    pub job: Option<OpenJob>,
    /// 工程预检已经结束，但首页偏好的真实写入未收口前，不允许候选读取旧配置。
    pub prepared: Option<Project>,
    pub picker: Option<Picker>,
    pub current: Option<AuthorProcess>,
    pub candidate: Option<AuthorProcess>,
    pub retiring: Option<(AuthorProcess, Instant)>,
    pub want_close: bool,
    pub close_sent: bool,
    pub exit_app: bool,
    pub notice: String,
    pub changed: bool,
    pub initial: Option<PathBuf>,
}

impl Host {
    pub fn busy(&self) -> bool {
        self.job.is_some()
            || self.prepared.is_some()
            || self.preferences_job.is_some()
            || self.picker.is_some()
            || self.candidate.is_some()
            || self.retiring.is_some()
            || self.want_close
    }
    pub fn notice(&mut self, notice: impl Into<String>) {
        self.notice = notice.into();
        self.changed = true;
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        if let Some(job) = &self.job {
            job.control.cancel();
        }
        if let Some(picker) = &self.picker {
            picker.picker.cancel();
        }
        // 正常退出前已等待拥有的实例收口；异常路径仅通知关闭，不杀死含未保存内容的当前作者。
        if let Some(current) = &self.current {
            let _ = current.send(super::types::HostEvent::RequestClose);
        }
        if let Some(candidate) = &self.candidate {
            let _ = candidate.send(super::types::HostEvent::AbortCandidate);
        }
    }
}
