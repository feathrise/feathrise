//! 唯一帮助浮层、原正文家节点及分组问号标记，正文关闭后回到原父级。

use super::types::HelpTarget;
use bevy::prelude::*;

pub(super) use feathrise_editor_ui::context_help::{ContextHelpBody, ContextHelpButton};
#[derive(Component)]
pub(super) struct ContextHelpHome {
    pub parent: Entity,
    pub target: HelpTarget,
}
/// 侧栏旧呈现只隐藏原位正文；移挂后由帮助域独占其Display，稳定帧不来回写布局。
#[derive(Component)]
pub(crate) struct ContextHelpMounted;
#[derive(Component, Clone, Default)]
pub(crate) struct ContextHelpPopup;
#[derive(Component, Clone, Default)]
pub(super) struct ContextHelpTitle;
#[derive(Component, Clone, Default)]
pub(super) struct ContextHelpScroll;
#[derive(Component, Clone, Default)]
pub(super) struct ContextHelpClose;
