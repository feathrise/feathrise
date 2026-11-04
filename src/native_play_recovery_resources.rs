//! 失败恢复夹具的唯一后台任务槽和有限阶段；普通会话不安装此资源。

use super::types::{AuthorSnapshot, Phase, RecoveryReport};
use crate::project::Project;
use bevy::{prelude::*, tasks::Task};
use std::{sync::Arc, time::Instant};

#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct RecoverySet;

#[derive(Resource)]
pub(super) struct Recovery {
    pub project: Project,
    pub original: Arc<Vec<u8>>,
    pub injected: Arc<Vec<u8>>,
    pub phase: Phase,
    pub started: Instant,
    pub baseline_started: u64,
    pub baseline_completed: u64,
    pub pulse: Option<Entity>,
    pub requested_surface: Option<String>,
    pub snapshot: Option<AuthorSnapshot>,
    pub io: Option<Task<Result<(), String>>>,
    pub error: Option<String>,
    pub cancel_sent: bool,
    pub report: RecoveryReport,
}
