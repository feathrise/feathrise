//! 绘制范围面板组件；绘制组与网格内部竞争分组使用不同动作。
use bevy::prelude::*;
use feathrise_scene_content::TerrainId;
#[derive(Component, Clone, Default)]
pub(super) struct PaintPanel;
#[derive(Component, Clone)]
pub(super) enum PaintAction {
    Current,
    Cross,
    New,
    Delete,
    Bind,
    Group(String),
    Target(TerrainId),
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ListKind {
    Groups,
    Targets,
}
impl ListKind {
    pub(super) fn slot(self) -> usize {
        if self == Self::Groups { 0 } else { 1 }
    }
}
#[derive(Component)]
pub(super) struct PaintList(pub ListKind);
#[derive(Component)]
pub(super) struct PaintListContent(pub ListKind);
#[derive(Component)]
pub(super) struct PaintRow {
    pub kind: ListKind,
    pub slot: usize,
}
#[derive(Component)]
pub(super) struct PaintRowText {
    pub kind: ListKind,
    pub slot: usize,
}
