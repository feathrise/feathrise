//! 后台消息、请求票据和单插件状态合同；只保存有界数据与通道，不持有动态库指针。
use feathrise_plugin_api::{Request, Snapshot};
use std::collections::VecDeque;

pub(super) enum Event {
    Loaded {
        index: usize,
        domain: &'static str,
        snapshot: Snapshot,
        requests: async_channel::Sender<Job>,
    },
    Updated {
        index: usize,
        sequence: u64,
        activate: bool,
        // 回执保留发起动作，保存成功只依据对应文档的saved_sequence确认。
        request: Request,
        snapshot: Result<Snapshot, String>,
    },
    Failed { id: String, error: String },
    ManagementWarning(String),
    Discovered {
        count: usize,
        states: std::collections::BTreeMap<String, String>,
        origins: std::collections::BTreeMap<String, super::management::types::PluginOrigin>,
        warnings: Vec<String>,
    },
}
pub(super) struct Job {
    pub sequence: u64,
    pub request: Request,
    pub activate: bool,
    /// 场景加载、地形跟随等后台事务直接收取有界C ABI回执，不能等待主线程事件转发。
    pub reply: Option<std::sync::mpsc::SyncSender<Result<Snapshot, String>>>,
}
pub(super) struct Tool {
    pub domain: &'static str,
    pub snapshot: Snapshot,
    pub requests: async_channel::Sender<Job>,
    pub queue: VecDeque<Job>,
    pub inflight: Option<u64>,
    pub completed: u64,
    pub sequence: u64,
    pub fault: Option<String>,
    pub library_open: bool,
    pub close_confirmation: Option<u32>,
    pub choices: Option<String>,
    pub latest_activation: u64,
    pub scene_pending: bool,
}
impl Tool {
    pub fn cancel_queued(&mut self) {
        self.completed = self.sequence;
        self.queue.clear();
    }
    pub fn busy(&self) -> bool {
        self.scene_pending || self.inflight.is_some() || !self.queue.is_empty()
    }
    pub fn send(&mut self, request: Request, activate: bool) -> Result<u64, String> {
        if self.scene_pending {
            return Err("场景作者正在采用完整版本，请稍候再编辑".into());
        }
        if let Some(error) = &self.fault {
            return Err(error.clone());
        }
        if self.queue.len() >= 8 {
            return Err("插件任务队列已满，请等待当前操作完成".into());
        }
        // 丢弃确认只对当时那份草稿有效；再次编辑或切标签后必须重新确认。
        if matches!(
            &request,
            Request::Edit { .. } | Request::Activate { .. } | Request::Save { .. }
        ) || matches!(&request, Request::Action { action, .. } if !["menu", "section", "preview", "templates", "close_templates"].contains(&action.as_str()))
        {
            self.close_confirmation = None;
        }
        self.sequence = self.sequence.checked_add(1).ok_or("插件请求身份耗尽")?;
        if activate {
            self.latest_activation = self.sequence;
        }
        self.queue.push_back(Job {
            sequence: self.sequence,
            request,
            activate,
            reply: None,
        });
        Ok(self.sequence)
    }
}
