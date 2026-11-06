//! 视口模式的进程内意图和确认；跨进程生命周期只消费已受理的模式变更。

use super::ViewportMode;
use bevy::prelude::*;

#[derive(Message, Debug, Clone, Copy)]
pub(crate) struct RequestViewportMode(pub ViewportMode);

#[derive(Message, Debug, Clone, Copy)]
pub(super) struct RequestBuildProfile(pub feathrise_editor_core::ProjectBuildProfile);

#[derive(Message, Debug, Clone, Copy)]
pub(super) struct RequestBuildBackend(pub crate::native_play::ProjectBuildBackend);

#[derive(Message, Debug, Clone, Copy)]
// 消息只确认切换已受理；当前模式和代次统一从 ViewportModeState 读取。
pub(crate) struct ViewportModeChanged;
