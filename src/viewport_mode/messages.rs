//! 视口模式的进程内意图和确认；跨进程生命周期只消费已受理的模式变更。

use super::ViewportMode;
use bevy::prelude::*;

#[derive(Message, Debug, Clone, Copy)]
pub(crate) struct RequestViewportMode(pub ViewportMode);

#[derive(Message, Debug, Clone, Copy)]
pub(super) struct RequestBuildProfile(pub feathrise_editor_core::ProjectBuildProfile);

#[derive(Message, Debug, Clone, Copy)]
pub(crate) struct ViewportModeChanged {
    pub previous: ViewportMode,
    pub current: ViewportMode,
    pub revision: u64,
}
