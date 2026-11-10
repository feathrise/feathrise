//! 诊断定位只有一个在途计算和一个最新目标；旧任务完成前持续占槽，取消不增加并发。

use super::types::{PendingLocation, PositionResult, Reveal};
use bevy::{prelude::*, tasks::Task};

#[derive(Resource, Default)]
pub(super) struct SourceLocationState {
    pub ticket: u64,
    pub pending: Option<PendingLocation>,
    pub task: Option<Task<PositionResult>>,
    pub ready: Option<PositionResult>,
    pub reveal: Option<Reveal>,
}
