//! 固定诊断行记录真实结果修订和绝对索引；滚动换绑会撤销旧释放点击。

use bevy::prelude::*;

#[derive(Component)]
pub(super) struct DiagnosticsOverlay;
#[derive(Component)]
pub(super) struct DiagnosticsPane;
#[derive(Component)]
pub(super) struct DiagnosticsContent;
#[derive(Component)]
pub(super) struct DiagnosticsStatus;
#[derive(Component)]
pub(super) struct DiagnosticsFooter;
#[derive(Component)]
pub(super) struct DiagnosticsEmpty;
#[derive(Component)]
pub(super) struct DiagnosticsClose;

#[derive(Component)]
pub(super) struct DiagnosticRow {
    pub slot: usize,
    pub index: Option<usize>,
    pub revision: u64,
    pub window_revision: u64,
}

#[derive(Component)]
pub(super) struct DiagnosticRowLabel {
    pub slot: usize,
    pub message: bool,
}
