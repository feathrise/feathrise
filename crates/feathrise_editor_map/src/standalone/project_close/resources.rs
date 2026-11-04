//! 仅独立编辑器持有一条工程保存队列；取消不丢弃原有在途任务句柄。

use super::types::{ProjectAuthorAdapter, ProjectSaveProgress, SaveItem};
use bevy::prelude::*;
use std::collections::VecDeque;

/// 地表仍是普通编辑器作者；完整实现由宿主注入，地图库不反向引用其实现包。
#[derive(Resource, Clone, Copy)]
pub struct ProjectCloseSaveAdapters {
    pub surface: fn(&mut World, u64) -> Result<(), String>,
}

/// 当前应用装配的外部作者事务；固定上限避免错误装配不断追加回调。
#[derive(Resource, Default)]
pub struct ProjectAuthorAdapters {
    pub(super) entries: Vec<ProjectAuthorAdapter>,
}

impl ProjectAuthorAdapters {
    pub fn register(&mut self, adapter: ProjectAuthorAdapter) -> Result<(), String> {
        if adapter.domain.is_empty()
            || self
                .entries
                .iter()
                .any(|item| item.domain == adapter.domain)
        {
            return Err(format!("作者保存域为空或重复：{}", adapter.domain));
        }
        if self.entries.len() >= 16 {
            return Err("作者保存参与者超过当前装配上限16，原队列保持".into());
        }
        self.entries.push(adapter);
        self.entries.sort_by_key(|item| (item.order, item.domain));
        Ok(())
    }
}

#[derive(Resource, Default)]
pub(super) struct ProjectSaveState {
    pub progress: ProjectSaveProgress,
    pub queue: VecDeque<SaveItem>,
    pub active: Option<ActiveSave>,
    pub failed_active: Option<ActiveSave>,
    pub cancel_after_active: bool,
}

#[derive(Clone)]
pub(super) struct ActiveSave {
    pub item: SaveItem,
    pub stage: SaveStage,
    pub catalog_before: Option<String>,
}

#[derive(Clone, Copy)]
pub(super) enum SaveStage {
    SelectingMap,
    Submitted { map_sequence: Option<u64> },
}
