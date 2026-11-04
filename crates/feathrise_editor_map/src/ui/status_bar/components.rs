//! 底栏、非模态详情浮层与独立滚动区的固定组件。

use bevy::prelude::*;

use super::types::{StatusAction, StatusTextRole};

pub(crate) use feathrise_editor_ui::PanelStatusText; // 领域状态在分库中声明，底栏统一采用同一标记。

#[derive(Component, Clone, Default)]
pub(super) struct StatusFooter;

/// 普通状态与切图进度共用底栏空间；详情入口独立于此容器，切图时仍可访问。
#[derive(Component, Clone, Default)]
pub(super) struct StatusFooterContent;

#[derive(Component, Clone, Default)]
pub(crate) struct StatusPopup;

#[derive(Component, Clone, Default)]
pub(super) struct StatusPopupHeader;

#[derive(Component, Clone, Default)]
pub(super) struct StatusPopupFooter;

#[derive(Component, Clone, Default)]
pub(super) struct StatusDetailScroll;

#[derive(Component, Clone, Default)]
pub(super) struct StatusText(pub(super) StatusTextRole);

#[derive(Component, Clone, Default)]
pub(super) struct StatusButton(pub(super) StatusAction);
