//! 两个宿主共用的固定批量保存结果节点；无独立正文或输入实体。

use bevy::prelude::*;

#[derive(Component, Clone, Default)]
pub struct SaveReport;
#[derive(Component, Clone, Default)]
pub struct SaveReportScroll;
#[derive(Component, Clone, Default)]
pub struct SaveReportSummary;
#[derive(Component, Clone, Default)]
pub struct SaveReportBody;
#[derive(Component, Clone, Default)]
pub struct HideSaveReport;
