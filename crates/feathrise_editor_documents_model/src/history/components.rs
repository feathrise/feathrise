//! 文档撤销与重做按钮身份，与场景作者动作保持独立。
use bevy::prelude::*;

#[derive(Component, Clone, Copy, Default, PartialEq, Eq)]
pub enum HistoryAction {
    #[default]
    Undo,
    Redo,
}
