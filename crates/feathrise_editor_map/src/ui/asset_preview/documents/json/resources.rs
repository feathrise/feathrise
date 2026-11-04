//! 文档资源内的唯一JSON任务槽和至多八份轻量校验缓存；不为每次输入新增任务。

use super::types::*;
use bevy::tasks::Task;

#[derive(Default)]
pub(in super::super) struct JsonTools {
    pub(super) task: Option<Task<WorkResult>>,
    pub(super) cache: Vec<Validation>,
    pub(super) save: Option<JsonKey>, // 一个显式保存意图；校验期间改字则取消，不能隐式保存新草稿。
    pub(super) format: Option<FormatRequest>,
    pub(super) ready: Option<WorkResult>,
    pub(super) locate: Option<JsonKey>,
    pub(super) reveal: Option<Reveal>,
    pub(super) ticket: u64,
}

impl JsonTools {
    pub(in super::super) fn saved_play_pending(&self) -> bool { self.save.is_some() }

    pub(in super::super) fn exit_pending(&self) -> bool {
        self.task.is_some() || self.save.is_some() || self.format.is_some() || self.ready.is_some()
    }

    pub(super) fn checked(&self, doc: &super::super::types::Document) -> Option<&Validation> {
        self.cache.iter().find(|entry| entry.key.current(doc))
    }
}
