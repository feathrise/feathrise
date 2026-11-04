//! 代码状态入口与显式采用按钮；不复用原启动按钮来隐式重启运行实例。

use bevy::prelude::*;

#[derive(Component)]
pub(super) struct CodeIterationControls;

#[derive(Component, Clone, Copy)]
pub(super) enum CodeIterationAction { Diagnostics, Apply }

#[derive(Component, Clone, Copy)]
pub(super) struct CodeIterationLabel(pub(super) CodeIterationAction);

#[derive(Component)]
pub(super) struct CodeIterationStatus;

#[derive(Component, Default)]
pub(super) struct ApplyVersion(pub(super) Option<u64>);
