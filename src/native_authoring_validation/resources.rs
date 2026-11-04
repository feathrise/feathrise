//! 显式验收唯一资源；保留有限工作阶段与证据，不拥有领域草稿或文件保存实现。

use super::types::{AuthoringReport, Phase, Pulse};
use bevy::prelude::Resource;
use std::time::Instant;

/// 仅验收入口安装；正常编辑器不构造此资源，也没有自动改作者内容的系统。
#[derive(Resource)]
pub(crate) struct NativeAuthoringValidation {
    pub(super) report: AuthoringReport,
    pub(super) edit: bool,
    pub(super) marker: String,
    pub(super) phase: Phase,
    pub(super) index: usize,
    pub(super) tab: Option<u64>,
    pub(super) pulse: Option<Pulse>,
    pub(super) started: Instant,
    pub(super) phase_frames: u16,
}
